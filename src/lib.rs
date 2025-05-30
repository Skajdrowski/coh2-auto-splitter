#![no_std]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![warn(
    clippy::complexity,
    clippy::correctness,
    clippy::perf,
    clippy::style,
    clippy::undocumented_unsafe_blocks,
    rust_2018_idioms
)]

use asr::{
    Address, Process, PointerSize, deep_pointer::DeepPointer,
    future::{next_tick, retry},
    settings::Gui,
    string::ArrayCString,
    timer::{self, TimerState},
    watcher::Watcher
};

asr::async_main!(stable);
asr::panic_handler!();

#[derive(Gui)]
struct Settings {
    #[default = false]
    Slow_PC_mode: bool
}

#[derive(Default)]
struct Watchers {
    loadByte: Watcher<u8>,
    isPausedByte: Watcher<u8>,
    promptByte: Watcher<u8>,
    level: Watcher<u8>,
    outro: Watcher<ArrayCString<5>>
}

struct Memory {
    load: Address,
    isPaused: DeepPointer<2>,
    prompt: Address,
    level: Address,
    outro: DeepPointer<4>
}

impl Memory {
    async fn init(process: &Process) -> Self {
        let baseModule = process.get_module_address("game.exe").expect("Failed to attach to the game.");
        let GameClient = retry(|| process.get_module_address("GameClient.dll")).await;

        //let baseModuleSize = retry(|| pe::read_size_of_image(process, baseModule)).await;
        //asr::print_limited::<128>(&format_args!("{}", baseModuleSize));

        Self { // v1.0
            load: GameClient + 0x219658,
            isPaused: DeepPointer::new(GameClient, PointerSize::Bit32, &[0x218F94, 0x58]),
            prompt: GameClient + 0x21CD6C,
            level: baseModule + 0x1C5159,
            outro: DeepPointer::new(GameClient, PointerSize::Bit32, &[0x220B10, 0x4, 0x4, 0x7])
        }
    }
}

fn start(watchers: &Watchers) -> bool {
    watchers.loadByte.pair.is_some_and(|val| val.changed_from_to(&3, &6))
}

fn isLoading(watchers: &Watchers) -> Option<bool> {
    Some(watchers.isPausedByte.pair?.current == 1 && watchers.loadByte.pair?.current == 1 && watchers.promptByte.pair?.current == 0 || watchers.loadByte.pair?.current == 3)
}

fn split(watchers: &Watchers) -> bool {
    watchers.level.pair.is_some_and(|val| val.changed_to(&0))
    || watchers.outro.pair.is_some_and(|val| val.current.matches("Outro"))
}

fn mainLoop(process: &Process, memory: &Memory, watchers: &mut Watchers) {
    watchers.isPausedByte.update_infallible(memory.isPaused.deref(process).unwrap_or(0));
    watchers.loadByte.update_infallible(process.read(memory.load).unwrap_or(0));
    watchers.promptByte.update_infallible(process.read(memory.prompt).unwrap_or(0));

    watchers.level.update_infallible(process.read(memory.level).unwrap_or(1));

    watchers.outro.update_infallible(memory.outro.deref(process).unwrap_or_default());
}

async fn main() {
    let mut settings = Settings::register();

    asr::set_tick_rate(60.0);
    let mut tickToggled = false;

    loop {
        let process = Process::wait_attach("game.exe").await;

        process.until_closes(async {
            let mut watchers = Watchers::default();
            let memory = Memory::init(&process).await;

            loop {
                settings.update();

                if settings.Slow_PC_mode && !tickToggled {
                    asr::set_tick_rate(30.0);
                    tickToggled = true;
                }
                else if !settings.Slow_PC_mode && tickToggled {
                    asr::set_tick_rate(60.0);
                    tickToggled = false;
                }

                if [TimerState::Running, TimerState::Paused].contains(&timer::state()) {
                    match isLoading(&watchers) {
                        Some(true) => timer::pause_game_time(),
                        Some(false) => timer::resume_game_time(),
                        _ => ()
                    }

                    if split(&watchers) {
                        timer::split();
                    }
                }

                if timer::state().eq(&TimerState::NotRunning) && start(&watchers) {
                    timer::start();
                }

                mainLoop(&process, &memory, &mut watchers);
                next_tick().await;
            }
        }).await;
    }
}