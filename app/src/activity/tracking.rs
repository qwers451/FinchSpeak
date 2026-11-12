pub trait Probe {
    type Process;
    fn foreground(&self) -> Option<(Self::Process, String)>;
    fn alive(&self, process: &Self::Process) -> bool;
}

pub struct Tracker<P> {
    active: Option<(P, String)>,
}

impl<P> Default for Tracker<P> {
    fn default() -> Self {
        Self { active: None }
    }
}

impl<P> Tracker<P> {
    pub fn poll(&mut self, probe: &impl Probe<Process = P>) -> Option<String> {
        if !self
            .active
            .as_ref()
            .is_some_and(|(process, _)| probe.alive(process))
        {
            self.active = probe.foreground();
        }
        self.active.as_ref().map(|(_, game)| game.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct FakeProbe {
        foreground: RefCell<Option<(u32, String)>>,
        live: Cell<bool>,
        calls: Cell<usize>,
    }

    impl Probe for FakeProbe {
        type Process = u32;
        fn foreground(&self) -> Option<(u32, String)> {
            self.calls.set(self.calls.get() + 1);
            self.foreground.borrow().clone()
        }
        fn alive(&self, _: &u32) -> bool {
            self.live.get()
        }
    }

    #[test]
    fn activity_survives_alt_tab_until_its_process_exits() {
        let probe = FakeProbe::default();
        probe.live.set(true);
        probe.foreground.replace(Some((1, "Portal".into())));
        let mut tracker = Tracker::default();
        assert_eq!(tracker.poll(&probe), Some("Portal".into()));
        probe.foreground.replace(Some((2, "Hades".into())));
        assert_eq!(tracker.poll(&probe), Some("Portal".into()));
        assert_eq!(
            probe.calls.get(),
            1,
            "Alt+Tab must retain the original process"
        );
        probe.live.set(false);
        probe.foreground.replace(None);
        assert_eq!(tracker.poll(&probe), None);
    }

    #[test]
    fn activity_denied_access_or_a_non_game_foreground_is_empty() {
        let mut tracker: Tracker<u32> = Tracker::default();
        assert_eq!(tracker.poll(&FakeProbe::default()), None);
    }
}
