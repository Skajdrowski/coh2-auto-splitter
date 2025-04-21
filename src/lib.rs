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
    Address, Process, PointerSize,
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
    cutsByte: Watcher<u8>,
    loadByte: Watcher<u8>,
    load2Byte: Watcher<u8>,
    level: Watcher<ArrayCString<2>>,
    outro: Watcher<ArrayCString<7>>
}

struct Memory {
    baseModule: Address,
    GameClient: Address,
    cuts: Address,
    load: Address,
    load2: [u64; 6],
    level: Address,
    outro: [u64; 4]
}

impl Memory {
    async fn init(process: &Process) -> Self {
        let baseModule = process.get_module_address("game.exe").expect("Failed to attach to the game.");
        let GameClient = retry(|| process.get_module_address("GameClient.dll")).await;

        //let baseModuleSize = retry(|| pe::read_size_of_image(process, baseModule)).await;
        //asr::print_limited::<128>(&format_args!("{}", baseModuleSize));

        Self { // v1.0
            baseModule,
            GameClient,
            cuts: GameClient + 0x21F050,
            load: baseModule + 0x1B9BF8,
            load2: [0x1CBD98, 0x4, 0x50, 0x50, 0x6C, 0x4],
            level: baseModule + 0x1C5159,
            outro: [0x220B10, 0x4, 0x4, 0x7]
        }
    }
}

fn start(watchers: &Watchers) -> bool {
    watchers.cutsByte.pair.is_some_and(|val| val.changed_from_to(&0, &1))
    && watchers.level.pair.is_some_and(|val| !val.current.is_empty())
}

fn isLoading(watchers: &Watchers) -> Option<bool> {
    Some(watchers.loadByte.pair?.current == 0 || watchers.load2Byte.pair?.current == 0)
}

fn split(watchers: &Watchers) -> bool {
        watchers.level.pair.is_some_and(|val|
            val.changed()
            && !val.current.is_empty()
        )
        || watchers.outro.pair.is_some_and(|val| 
        val.old.matches("Outro_2")
        && val.changed()
        )
}

fn mainLoop(process: &Process, memory: &Memory, watchers: &mut Watchers) {
    watchers.cutsByte.update_infallible(process.read(memory.cuts).unwrap_or_default());

    watchers.loadByte.update_infallible(process.read(memory.load).unwrap_or(1));
    watchers.load2Byte.update_infallible(process.read_pointer_path(memory.baseModule, PointerSize::Bit32, &memory.load2).unwrap_or(1));

    watchers.level.update_infallible(process.read(memory.level).unwrap_or_default());

    watchers.outro.update_infallible(process.read_pointer_path(memory.GameClient, PointerSize::Bit32, &memory.outro).unwrap_or_default());
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