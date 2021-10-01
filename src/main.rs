#![allow(unused_imports)]
#![feature(const_mut_refs)]

use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Condvar, Mutex};
use std::{env, sync::atomic::*, sync::Arc, thread, time::*};

use anyhow::*;
use log::*;

use url;

use core::ptr;

use embedded_svc::anyerror::*;
use embedded_svc::httpd::registry::*;
use embedded_svc::httpd::*;
use embedded_svc::ping::Ping;
use embedded_svc::wifi::*;

use esp_idf_svc::httpd as idf;
use esp_idf_svc::httpd::ServerRegistry;
use esp_idf_svc::netif::*;
use esp_idf_svc::nvs::*;
use esp_idf_svc::ping;
use esp_idf_svc::sysloop::*;
use esp_idf_svc::wifi::*;

use esp_idf_hal::delay;
use esp_idf_hal::gpio;
use esp_idf_hal::i2c;
use esp_idf_hal::prelude::*;
use esp_idf_hal::spi;
use esp_idf_hal::ulp;

use esp_idf_sys;
use esp_idf_sys::esp;

use esp_homekit_sdk_sys::*;

use esp32_hal::prelude::*;

use esp32_hal::clock_control::{sleep, CPUSource::PLL, ClockControl};
use esp32_hal::dport::Split;
use esp32_hal::dprintln;
use esp32_hal::interrupt::{clear_software_interrupt, Interrupt, InterruptLevel};
use esp32_hal::serial::{config::Config, Serial};
use esp32_hal::target;
use esp32_hal::Core::PRO;
use esp32_hal::*;

use display_interface_spi::SPIInterfaceNoCS;

use core::cell;

use lazy_static::lazy_static;

const SSID: &str = "ssid";
const PASS: &str = "password";

const SMART_OUTLET_TASK_NAME: &str = "hap_outlet";
const SMART_OUTLET_TASK_STACKSIZE: u32 = 40000;
const SMART_OUTLET_TASK_PRIORITY: UBaseType_t = 1;

static GPIO: CriticalSectionSpinLockMutex<
    Option<esp_idf_hal::gpio::Gpio26<esp_idf_hal::gpio::Output>>,
> = CriticalSectionSpinLockMutex::new(None);

fn main() -> Result<()> {
    test_atomics();

    test_threads();

    let task = smart_outlet_handler;

    task::Task::create(
        task,
        SMART_OUTLET_TASK_NAME,
        SMART_OUTLET_TASK_STACKSIZE,
        SMART_OUTLET_TASK_PRIORITY,
    );

    Ok(())
}

fn smart_outlet_handler(cv: *mut esp_homekit_sdk_sys::c_types::c_void) {
    env::set_var("RUST_BACKTRACE", "1"); // Get some nice backtraces from Anyhow

    let mut wifi = wifi();

    //let mutex = Arc::new((Mutex::new(None), Condvar::new()));
    //let httpd = httpd(mutex.clone());

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;
    let mut switch = pins.gpio26.into_output().unwrap();
    switch.set_low();

    use esp32_hal::gpio::Mutex;
    (&GPIO).lock(|val| *val = Some(switch));

    let hap_config = hap::Config {
        name: CString::new("Smart-Outlet").unwrap(),
        model: CString::new("Esp32").unwrap(),
        manufacturer: CString::new("Espressif").unwrap(),
        serial_num: CString::new("111122334455").unwrap(),
        fw_rev: CString::new("1.0.0").unwrap(),
        hw_rev: CString::new("0.1.0").unwrap(),
        pv: CString::new("1.1.0").unwrap(),
        cid: accessory::Category::OUTLET,
    };

    hap::init();

    let mut accessory = accessory::create(&hap_config);
    let mut service = service::create();

    service::add_name(service, "My Smart Outlet");

    let outlet_in_use = service::get_service_by_uuid(service);

    service::set_write_cb(service, Some(outlet_write));

    hap::add_service_to_accessory(accessory, service);

    hap::add_accessory(accessory);

    let setup_code = CString::new("111-22-333").unwrap();
    let setup_id = CString::new("ES32").unwrap();

    hap::secret(setup_code, setup_id);

    hap::start();

    //let mut wait = mutex.0.lock().unwrap();

    loop {}
    //     #[allow(unused)]
    //     let cycles = loop {
    //         if let Some(cycles) = *wait {
    //             break cycles;
    //         } else {
    //             wait = mutex.1.wait(wait).unwrap();
    //         }
    //     };

    for s in 0..3 {
        info!("Shutting down in {} secs", 3 - s);
        thread::sleep(Duration::from_secs(1));
    }

    drop(httpd);
    info!("Httpd stopped");

    drop(wifi);
    info!("Wifi stopped");
}

unsafe extern "C" fn outlet_write(
    write_data: *mut esp_homekit_sdk_sys::hap_write_data_t,
    count: i32,
    serv_priv: *mut esp_homekit_sdk_sys::c_types::c_void,
    write_priv: *mut esp_homekit_sdk_sys::c_types::c_void,
) -> i32 {
    use esp32_hal::gpio::Mutex;

    let mut gpio = &GPIO;

    if (*write_data).val.b == true {
        gpio.lock(|gpio| {
            let gpio = gpio.as_mut().unwrap();
            gpio.set_high();
        })
    } else {
        gpio.lock(|gpio| {
            let gpio = gpio.as_mut().unwrap();
            gpio.set_low();
        })
    }

    hap::HAP_SUCCESS_
}

fn test_threads() {
    let mut children = vec![];

    println!("Rust main thread: {:?}", thread::current());

    for i in 0..5 {
        // Spin up another thread
        children.push(thread::spawn(move || {
            println!("This is thread number {}, {:?}", i, thread::current());
        }));
    }

    println!(
        "About to join the threads. If ESP-IDF was patched successfully, joining will NOT crash"
    );

    for child in children {
        // Wait for the thread to finish. Returns a result.
        let _ = child.join();
    }

    thread::sleep(Duration::from_secs(2));

    println!("Joins were successful.");
}

#[allow(deprecated)]
fn test_atomics() {
    let a = AtomicUsize::new(0);
    let v1 = a.compare_and_swap(0, 1, Ordering::SeqCst);
    let v2 = a.swap(2, Ordering::SeqCst);

    let (r1, r2) = unsafe {
        // don't optimize our atomics out
        let r1 = core::ptr::read_volatile(&v1);
        let r2 = core::ptr::read_volatile(&v2);

        (r1, r2)
    };

    println!("Result: {}, {}", r1, r2);
}

#[allow(unused_variables)]
fn httpd(mutex: Arc<(Mutex<Option<u32>>, Condvar)>) -> Result<idf::Server> {
    let server = idf::ServerRegistry::new()
        .at("/")
        .get(|_| Ok("Hello, world!".into()))?
        .at("/foo")
        .get(|_| bail!("Boo, something happened!"))?
        .at("/bar")
        .get(|_| {
            Response::new(403)
                .status_message("No permissions")
                .body("You have no permissions to access this page".into())
                .into()
        })?;

    server.start(&Default::default())
}

fn wifi() -> Result<Box<EspWifi>> {
    let mut wifi = Box::new(EspWifi::new(
        Arc::new(EspNetifStack::new()?),
        Arc::new(EspSysLoopStack::new()?),
        Arc::new(EspDefaultNvs::new()?),
    )?);

    info!("Wifi created, about to scan");

    let ap_infos = wifi.scan()?;

    let ours = ap_infos.into_iter().find(|a| a.ssid == SSID);

    let channel = if let Some(ours) = ours {
        info!(
            "Found configured access point {} on channel {}",
            SSID, ours.channel
        );
        Some(ours.channel)
    } else {
        info!(
            "Configured access point {} not found during scanning, will go with unknown channel",
            SSID
        );
        None
    };

    wifi.set_configuration(&Configuration::Mixed(
        ClientConfiguration {
            ssid: SSID.into(),
            password: PASS.into(),
            channel,
            ..Default::default()
        },
        AccessPointConfiguration {
            ssid: "aptest".into(),
            channel: channel.unwrap_or(1),
            ..Default::default()
        },
    ))?;

    info!("Wifi configuration set, about to get status");

    let status = wifi.get_status();

    if let Status(
        ClientStatus::Started(ClientConnectionStatus::Connected(ClientIpStatus::Done(ip_settings))),
        ApStatus::Started(ApIpStatus::Done),
    ) = status
    {
        info!("Wifi connected, about to do some pings");

        let ping_summary =
            ping::EspPing::default().ping(ip_settings.subnet.gateway, &Default::default())?;
        if ping_summary.transmitted != ping_summary.received {
            bail!(
                "Pinging gateway {} resulted in timeouts",
                ip_settings.subnet.gateway
            );
        }

        info!("Pinging done");
    } else {
        bail!("Unexpected Wifi status: {:?}", status);
    }

    Ok(wifi)
}

pub fn from_cstr(buf: &[u8]) -> std::borrow::Cow<'_, str> {
    // We have to find the first '\0' ourselves, because the passed buffer might
    // be wider than the ASCIIZ string it contains
    let len = buf.iter().position(|e| *e == 0).unwrap() + 1;

    unsafe { CStr::from_bytes_with_nul_unchecked(&buf[0..len]) }.to_string_lossy()
}
