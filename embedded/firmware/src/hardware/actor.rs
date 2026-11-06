use core::pin::pin;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver, Watch};
use embassy_time::Instant;
use smart_leds::RGB8;

use crate::alarm::model::Alarm;
use crate::buzzer::BuzzerDriver;
use crate::led::LedDriver;
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};

/// The display
const SUBSCRIBERS: usize = 1;
static STATE: Watch<CriticalSectionRawMutex, HardwareState, SUBSCRIBERS> = Watch::new();

/// Receives every new hardware state
pub fn subscribe() -> Receiver<'static, CriticalSectionRawMutex, HardwareState, SUBSCRIBERS> {
    STATE
        .receiver()
        .expect("more hardware state subscribers than SUBSCRIBERS allows")
}

static MAILBOX: Mailbox<Program> = Mailbox::new();

/// What the hardware is playing, as published to subscribers
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HardwareState {
    Init,
    Idle,
    Alarm,
    Color,
    Fire,
    Rainbow,
}

impl HardwareState {
    pub fn label(self) -> &'static str {
        match self {
            HardwareState::Init => "Init...",
            HardwareState::Idle => "Idle",
            HardwareState::Alarm => "Alarm",
            HardwareState::Color => "Color",
            HardwareState::Fire => "Fire",
            HardwareState::Rainbow => "Rainbow",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Program {
    /// Runs the LEDs and buzzer once at boot
    Init,
    Idle,
    /// `elapsed_secs` into the alarm at `at`, so a resumed alarm picks up where it was
    Alarm {
        alarm: Alarm,
        elapsed_secs: u32,
        at: Instant,
    },
    Color(RGB8),
    Fire,
    Rainbow,
}

impl Program {
    fn state(&self) -> HardwareState {
        match self {
            Program::Init => HardwareState::Init,
            Program::Idle => HardwareState::Idle,
            Program::Alarm { .. } => HardwareState::Alarm,
            Program::Color(_) => HardwareState::Color,
            Program::Fire => HardwareState::Fire,
            Program::Rainbow => HardwareState::Rainbow,
        }
    }

    /// The same alarm asked for again, from a different start, keeps playing
    fn is_same(&self, other: &Program) -> bool {
        match (self, other) {
            (Program::Alarm { alarm: a, .. }, Program::Alarm { alarm: b, .. }) => a == b,
            _ => self == other,
        }
    }
}

#[derive(Clone, Copy)]
pub struct HardwareHandle {
    mailbox: MailboxSender<Program>,
}

pub fn handle() -> HardwareHandle {
    HardwareHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl HardwareHandle {
    pub fn idle(&self) {
        self.request(Program::Idle);
    }

    pub fn color(&self, color: RGB8) {
        self.request(Program::Color(color));
    }

    pub fn fire(&self) {
        self.request(Program::Fire);
    }

    pub fn rainbow(&self) {
        self.request(Program::Rainbow);
    }

    /// `elapsed_secs` is how far into the alarm we already are, when resuming one
    /// Waits for room rather than dropping the alarm
    pub async fn start_alarm(&self, alarm: Alarm, elapsed_secs: u32) {
        let program = Program::Alarm {
            alarm,
            elapsed_secs,
            at: Instant::now(),
        };
        self.mailbox.send(program).await;
    }

    fn request(&self, program: Program) {
        self.mailbox.post_or_warn(program);
    }
}

/// Drives the LEDs and buzzer for the requested state
struct HardwareActor {
    led_driver: LedDriver,
    buzzer_driver: BuzzerDriver,
    mailbox: MailboxReceiver<Program>,
    program: Program,
}

impl HardwareActor {
    async fn run(&mut self) -> ! {
        loop {
            STATE.sender().send(self.program.state());
            let next = self.play_until_new().await;
            log::info!(
                "Hardware: {} -> {}",
                self.program.state().label(),
                next.state().label()
            );
            self.program = next;
        }
    }

    /// Plays the current program until it finishes or a request replaces it. A request for the
    /// current program leaves it playing undisturbed.
    async fn play_until_new(&mut self) -> Program {
        let mut playing = pin!(play(
            &mut self.led_driver,
            &mut self.buzzer_driver,
            &self.program
        ));
        loop {
            match select(&mut playing, self.mailbox.receive()).await {
                Either::First(()) => return Program::Idle,
                Either::Second(next) if !next.is_same(&self.program) => {
                    if let Program::Alarm { alarm, .. } = &self.program {
                        log::info!("Alarm '{}' cancelled", alarm.name);
                    }
                    return next;
                }
                Either::Second(_) => {}
            }
        }
    }
}

/// Returns once a program with an end has finished; the others play until dropped
async fn play(led: &mut LedDriver, buzzer: &mut BuzzerDriver, program: &Program) {
    match program {
        Program::Init => {
            log::info!("Testing LED driver...");
            led.run_wheel_once().await;
            log::info!("Testing BUZZER driver...");
            buzzer.play_short_melody().await;
            log::info!("All OK");
        }
        Program::Alarm {
            alarm,
            elapsed_secs,
            at,
        } => {
            buzzer.off();
            let elapsed_secs = elapsed_secs + at.elapsed().as_secs() as u32;
            led.follow_intensity_curve(
                &alarm.color,
                &alarm.intensity_curve,
                alarm.length,
                elapsed_secs,
            )
            .await;
            if alarm.play_melody {
                join(led.blink(&alarm.color), buzzer.play_melody()).await;
            } else {
                led.blink(&alarm.color).await;
            }
            log::info!("Alarm '{}' completed", alarm.name);
        }
        Program::Idle => {
            buzzer.off();
            led.color(RGB8::default()).await
        }
        Program::Color(color) => {
            buzzer.off();
            led.color(*color).await
        }
        Program::Fire => {
            buzzer.off();
            led.fire().await
        }
        Program::Rainbow => {
            buzzer.off();
            led.rainbow().await
        }
    }
}

pub fn spawn(spawner: Spawner, led_driver: LedDriver, buzzer_driver: BuzzerDriver) {
    let actor = HardwareActor {
        led_driver,
        buzzer_driver,
        mailbox: MAILBOX.receiver(),
        program: Program::Init,
    };
    spawner.spawn(hardware_task(actor).unwrap());
}

#[embassy_executor::task]
async fn hardware_task(mut actor: HardwareActor) -> ! {
    actor.run().await
}
