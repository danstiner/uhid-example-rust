/*
 * UHID Example
 *
 * Copyright (c) 2012-2013 David Herrmann <dh.herrmann@gmail.com>
 *
 * Converted from c to rust by Daniel Stiner <daniel.stiner@gmail.com>
 *
 * The code may be used by anyone for any purpose,
 * and can serve as a starting point for developing
 * applications using uhid.
 */

/*
 * UHID Example
 * This example emulates a basic 3 buttons mouse with wheel over UHID. Run this
 * program as root and then use the following keys to control the mouse:
 *   q: Quit the application
 *   1: Toggle left button (down, up, ...)
 *   2: Toggle right button
 *   3: Toggle middle button
 *   a: Move mouse left
 *   d: Move mouse right
 *   w: Move mouse up
 *   s: Move mouse down
 *   r: Move wheel up
 *   f: Move wheel down
 *
 * Additionally to 3 button mouse, 3 keyboard LEDs are also supported (LED_NUML,
 * LED_CAPSL and LED_SCROLLL). The device doesn't generate any related keyboard
 * events, though. You need to manually write the EV_LED/LED_XY/1 activation
 * input event to the evdev device to see it being sent to this device.
 *
 * If uhid is not available as /dev/uhid, then you can pass a different path as
 * first argument.
 * If <linux/uhid.h> is not installed in /usr, then compile this with:
 *   gcc -o ./uhid_test -Wall -I./include ./samples/uhid/uhid-example.c
 * And ignore the warning about kernel headers. However, it is recommended to
 * use the installed uhid.h if available.
 */

extern crate libc;
extern crate mio;
extern crate nix;
extern crate termios;

use mio::{Events, Poll, PollOpt, Ready, Token};
use mio::unix::EventedFd;
use nix::fcntl;
use nix::unistd;
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::mem;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;
use std::process;
use std::slice;
use termios::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/*
 * HID Report Desciptor
 * We emulate a basic 3 button mouse with wheel and 3 keyboard LEDs. This is
 * the report-descriptor as the kernel will parse it:
 *
 * INPUT(1)[INPUT]
 *   Field(0)
 *     Physical(GenericDesktop.Pointer)
 *     Application(GenericDesktop.Mouse)
 *     Usage(3)
 *       Button.0001
 *       Button.0002
 *       Button.0003
 *     Logical Minimum(0)
 *     Logical Maximum(1)
 *     Report Size(1)
 *     Report Count(3)
 *     Report Offset(0)
 *     Flags( Variable Absolute )
 *   Field(1)
 *     Physical(GenericDesktop.Pointer)
 *     Application(GenericDesktop.Mouse)
 *     Usage(3)
 *       GenericDesktop.X
 *       GenericDesktop.Y
 *       GenericDesktop.Wheel
 *     Logical Minimum(-128)
 *     Logical Maximum(127)
 *     Report Size(8)
 *     Report Count(3)
 *     Report Offset(8)
 *     Flags( Variable Relative )
 * OUTPUT(2)[OUTPUT]
 *   Field(0)
 *     Application(GenericDesktop.Keyboard)
 *     Usage(3)
 *       LED.NumLock
 *       LED.CapsLock
 *       LED.ScrollLock
 *     Logical Minimum(0)
 *     Logical Maximum(1)
 *     Report Size(1)
 *     Report Count(3)
 *     Report Offset(0)
 *     Flags( Variable Absolute )
 *
 * This is the mapping that we expect:
 *   Button.0001 ---> Key.LeftBtn
 *   Button.0002 ---> Key.RightBtn
 *   Button.0003 ---> Key.MiddleBtn
 *   GenericDesktop.X ---> Relative.X
 *   GenericDesktop.Y ---> Relative.Y
 *   GenericDesktop.Wheel ---> Relative.Wheel
 *   LED.NumLock ---> LED.NumLock
 *   LED.CapsLock ---> LED.CapsLock
 *   LED.ScrollLock ---> LED.ScrollLock
 *
 * This information can be verified by reading /sys/kernel/debug/hid/<dev>/rdesc
 * This file should print the same information as showed above.
 */

const RDESC: [u8; 85] = [
    0x05, 0x01,	/* USAGE_PAGE (Generic Desktop) */
    0x09, 0x02,	/* USAGE (Mouse) */
    0xa1, 0x01,	/* COLLECTION (Application) */
    0x09, 0x01,		/* USAGE (Pointer) */
    0xa1, 0x00,		/* COLLECTION (Physical) */
    0x85, 0x01,			/* REPORT_ID (1) */
    0x05, 0x09,			/* USAGE_PAGE (Button) */
    0x19, 0x01,			/* USAGE_MINIMUM (Button 1) */
    0x29, 0x03,			/* USAGE_MAXIMUM (Button 3) */
    0x15, 0x00,			/* LOGICAL_MINIMUM (0) */
    0x25, 0x01,			/* LOGICAL_MAXIMUM (1) */
    0x95, 0x03,			/* REPORT_COUNT (3) */
    0x75, 0x01,			/* REPORT_SIZE (1) */
    0x81, 0x02,			/* INPUT (Data,Var,Abs) */
    0x95, 0x01,			/* REPORT_COUNT (1) */
    0x75, 0x05,			/* REPORT_SIZE (5) */
    0x81, 0x01,			/* INPUT (Cnst,Var,Abs) */
    0x05, 0x01,			/* USAGE_PAGE (Generic Desktop) */
    0x09, 0x30,			/* USAGE (X) */
    0x09, 0x31,			/* USAGE (Y) */
    0x09, 0x38,			/* USAGE (WHEEL) */
    0x15, 0x81,			/* LOGICAL_MINIMUM (-127) */
    0x25, 0x7f,			/* LOGICAL_MAXIMUM (127) */
    0x75, 0x08,			/* REPORT_SIZE (8) */
    0x95, 0x03,			/* REPORT_COUNT (3) */
    0x81, 0x06,			/* INPUT (Data,Var,Rel) */
    0xc0,			/* END_COLLECTION */
    0xc0,		/* END_COLLECTION */
    0x05, 0x01,	/* USAGE_PAGE (Generic Desktop) */
    0x09, 0x06,	/* USAGE (Keyboard) */
    0xa1, 0x01,	/* COLLECTION (Application) */
    0x85, 0x02,		/* REPORT_ID (2) */
    0x05, 0x08,		/* USAGE_PAGE (Led) */
    0x19, 0x01,		/* USAGE_MINIMUM (1) */
    0x29, 0x03,		/* USAGE_MAXIMUM (3) */
    0x15, 0x00,		/* LOGICAL_MINIMUM (0) */
    0x25, 0x01,		/* LOGICAL_MAXIMUM (1) */
    0x95, 0x03,		/* REPORT_COUNT (3) */
    0x75, 0x01,		/* REPORT_SIZE (1) */
    0x91, 0x02,		/* Output (Data,Var,Abs) */
    0x95, 0x01,		/* REPORT_COUNT (1) */
    0x75, 0x05,		/* REPORT_SIZE (5) */
    0x91, 0x01,		/* Output (Cnst,Var,Abs) */
    0xc0,		/* END_COLLECTION */
];

const DEFAULT_PATH: &str = "/dev/uhid";

#[derive(Clone, Copy)]
struct DeviceState {
    btn1_down: bool,
    btn2_down: bool,
    btn3_down: bool,
    abs_hor: i8,
    abs_ver: i8,
    wheel: i8,
}

impl Default for DeviceState {
    fn default() -> DeviceState {
        DeviceState {
            btn1_down: false,
            btn2_down: false,
            btn3_down: false,
            abs_hor: 0,
            abs_ver: 0,
            wheel: 0,
        }
    }
}

impl DeviceState {
    fn toggle_btn1(&mut self) {
        self.btn1_down = !self.btn1_down;
    }
    fn toggle_btn2(&mut self) {
        self.btn2_down = !self.btn2_down;
    }
    fn toggle_btn3(&mut self) {
        self.btn3_down = !self.btn3_down;
    }
}

fn uhid_write(file: &mut File, uhid_event: &uhid_event) -> io::Result<()> {
    let uhid_event_slice: &[u8];
    let uhid_event_size = mem::size_of::<uhid_event>();
    unsafe {
        uhid_event_slice = slice::from_raw_parts(
            uhid_event as *const _ as *const u8,
            uhid_event_size
        );
    }
    match file.write(uhid_event_slice) {
        Ok(bytes_written) =>
            if bytes_written != uhid_event_size {
                Err(io::Error::new(io::ErrorKind::Interrupted, format!("Wrong size written to uhid: {} != {}", bytes_written, uhid_event_size)))
            } else {
                Ok(())
            },
        Err(err) => Err(io::Error::new(err.kind(), format!("Cannot write to uhid: {}", err)))
    }
}

fn create(file: &mut File) -> io::Result<()> {
    let mut rdesc = RDESC;
    let mut ev: uhid_event = unsafe { mem::zeroed() };

    ev.type_ = uhid_event_type::__UHID_LEGACY_CREATE as u32;

    unsafe {
        let create = ev.u.create.as_mut();
        create.name.copy_from_slice(
            &[CString::new("test-uhid-device").unwrap().as_bytes_with_nul(), &[0u8; 111]].concat());
        create.rd_data = &mut rdesc[0] as *mut u8;
        create.rd_size = rdesc.len() as u16;
        create.bus = BUS_USB as u16;
        create.vendor = 0x15d9;
        create.product = 0x0a37;
        create.version = 0;
        create.country = 0;
    }

    uhid_write(file, &ev)
}

fn destroy(file: &mut File) -> io::Result<()>
{
    let mut ev: uhid_event = unsafe { mem::zeroed() };

    ev.type_ = uhid_event_type::UHID_DESTROY as u32;

    uhid_write(file, &ev)
}

fn send_event(file: &mut File, state: &DeviceState) -> io::Result<()> {
    let mut ev: uhid_event = unsafe { mem::zeroed() };

    ev.type_ = uhid_event_type::__UHID_LEGACY_INPUT as u32;

    unsafe {
        let input = ev.u.input.as_mut();
        input.size = 5;
        input.data[0] = 0x1;
        if state.btn1_down {
            input.data[1] |= 0x1;
        }
        if state.btn1_down {
            input.data[1] |= 0x2;
        }
        if state.btn1_down {
            input.data[1] |= 0x4;
        }
        input.data[2] = state.abs_hor as u8;
        input.data[3] = state.abs_ver as u8;
        input.data[4] = state.wheel as u8;
    }

    uhid_write(file, &ev)
}

fn keyboard(file: &mut File, state: &mut DeviceState) -> io::Result<()>
{
    let mut character: [u8; 1] = Default::default();
    io::stdin().read(&mut character)?;

    match character[0] {
        b'1' => {
            state.toggle_btn1();
            send_event(file, &state);
        },
        b'2' => {
            state.toggle_btn2();
            send_event(file, &state);
        },
        b'3' => {
            state.toggle_btn3();
            send_event(file, &state);
        },
        b'a' => {
            state.abs_hor = -20;
            send_event(file, &state);
            state.abs_hor = 0;
        },
        b'd' => {
            state.abs_hor = 20;
            send_event(file, &state);
            state.abs_hor = 0;
        },
        b'w' => {
            state.abs_ver = -20;
            send_event(file, &state);
            state.abs_ver = 0;
        },
        b's' => {
            state.abs_ver = 20;
            send_event(file, &state);
            state.abs_ver = 0;
        },
        b'r' => {
            state.wheel = 1;
            send_event(file, &state);
            state.wheel = 0;
        },
        b'f' => {
            state.wheel = -1;
            send_event(file, &state);
            state.wheel = 0;
        },
        b'q' => {
            return Err(io::Error::new(io::ErrorKind::Other, "Cancelled"));
        },
        c => eprintln!("Invalid input: {}", c as char)
    };

    Ok(())
}

fn main() {
    let mut device_state = Default::default();

    match Termios::from_fd(libc::STDIN_FILENO) {
        Err(_) => eprintln!("Cannot get tty state"),
        Ok(mut state) => {
            state.c_lflag &= !ICANON;
            state.c_cc[VMIN] = 1;
            match tcsetattr(libc::STDIN_FILENO, TCSANOW, &state) {
                Err(_) => eprintln!("Cannot set tty state"),
                Ok(_) => ()
            }
        }
    }

    let path = match env::args().nth(1) {
        Some(arg) => {
            if arg == "-h" || arg == "--help" {
                eprintln!("Usage: {} [{}]", env::args().nth(0).unwrap(), DEFAULT_PATH);
                return;
            } else {
                PathBuf::from(arg)
            }
        }
        None => PathBuf::from(DEFAULT_PATH)
    };

    eprintln!("Open uhid-cdev {}", path.to_str().unwrap());
    let fd = fcntl::open(&path, fcntl::O_RDWR | fcntl::O_CLOEXEC, nix::sys::stat::S_IRUSR | nix::sys::stat::S_IWUSR | nix::sys::stat::S_IRGRP | nix::sys::stat::S_IWGRP).map_err(|err| format!("Cannot open uhid-cdev {}: {}", path.to_str().unwrap(), err)).unwrap();
    let mut file = unsafe { File::from_raw_fd(fd) };

    eprintln!("Create uhid device");
    create(&mut file).unwrap();

    const STDIN: Token = Token(0);
    const UHID_DEVICE: Token = Token(1);

    let poll = Poll::new().unwrap();

    poll.register(&EventedFd(&libc::STDIN_FILENO), STDIN,
                  Ready::readable(), PollOpt::edge()).unwrap();
    poll.register(&EventedFd(&fd), UHID_DEVICE, Ready::readable(),
                  PollOpt::edge()).unwrap();

    let mut events = Events::with_capacity(1);

    println!("Press 'q' to quit...");
    loop {
        poll.poll(&mut events, None).map_err(|err| eprintln!("Cannot poll for fds: {}", err)).unwrap();

        for event in events.iter() {
            match event.token() {
                STDIN => {
                    keyboard(&mut file, &mut device_state);
                }
                UHID_DEVICE => {
                    // event()
                }
                _ => unreachable!(),
            }
        }
    }

    println!("Destroy uhid device");
    destroy(&mut file);
}
