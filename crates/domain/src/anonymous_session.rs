//! Server-owned anonymous activity. Public labels and request fields carry no
//! authority over these values. The deletion capability lives separately.

mod capability;
pub use capability::{Capability, Fingerprints};

pub const IDLE_RESET_SECONDS: u64 = 604_800;
pub const COOKIE_SECONDS: u64 = 31_536_000;
pub const ACTION_SECONDS: u64 = 14_400;
pub const NETWORK_CHANGE_SECONDS: u64 = 1_800;
const POST: u8 = 1;
const IMAGE: u8 = 2;
const THREAD: u8 = 4;
const REPORT: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Post { thread: bool, image: bool },
    Report,
}

impl Activity {
    fn bits(self) -> u8 {
        match self {
            Self::Post { thread, image } => {
                POST | if thread { THREAD } else { 0 } | if image { IMAGE } else { 0 }
            }
            Self::Report => REPORT,
        }
    }
}

/// Comparisons made by the server against the stored, session-specific hashes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub network: bool,
    pub address: bool,
    pub environment: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub created_at: u64,
    pub network_at: u64,
    pub address_at: u64,
    pub environment_at: u64,
    pub activity_at: u64,
    pub action_at: u64,
    pub verified_level: u8,
    pub posts: u8,
    pub images: u8,
    pub threads: u8,
    pub reports: u8,
    pub pending: u8,
    pub change_score: u8,
}

impl State {
    pub fn new(now: u64) -> Self {
        Self {
            created_at: now,
            network_at: now,
            address_at: now,
            environment_at: now,
            activity_at: 0,
            action_at: 0,
            verified_level: 0,
            posts: 0,
            images: 0,
            threads: 0,
            reports: 0,
            pending: 0,
            change_score: 0,
        }
    }

    /// The source retains the decoded password on idle expiry. Reset only the
    /// activity state; the caller must retain the existing opaque capability.
    /// Returns whether the idle reset occurred.
    pub fn resume(&mut self, now: u64, changes: Changes) -> bool {
        let last = if self.activity_at > 0 {
            self.activity_at
        } else {
            self.created_at
        };
        if now.saturating_sub(last) >= IDLE_RESET_SECONDS {
            *self = Self::new(now);
            // resetTimestamps sets action_ts; fresh generation leaves it zero.
            self.action_at = now;
            return true;
        }
        if changes.environment {
            self.environment_at = now;
        }
        if changes.network {
            self.network_at = now;
            self.address_at = now;
        } else if changes.address {
            self.address_at = now;
        }
        false
    }

    pub fn password_age(&self, now: u64) -> u64 {
        age(now, self.created_at)
    }

    pub fn network_age(&self, now: u64) -> u64 {
        age(now, self.network_at)
    }

    pub fn address_age(&self, now: u64) -> u64 {
        age(now, self.address_at)
    }

    pub fn environment_age(&self, now: u64) -> u64 {
        age(now, self.environment_at)
    }

    pub fn post_count(&self) -> u16 {
        u16::from(self.posts) + u16::from(self.pending & POST != 0)
    }

    pub fn image_count(&self) -> u16 {
        u16::from(self.images) + u16::from(self.pending & IMAGE != 0)
    }

    pub fn thread_count(&self) -> u16 {
        u16::from(self.threads) + u16::from(self.pending & THREAD != 0)
    }

    pub fn report_count(&self) -> u16 {
        u16::from(self.reports) + u16::from(self.pending & REPORT != 0)
    }

    pub fn is_known(&self, now: u64, minutes: u32, since: u64) -> bool {
        let network_age = self.network_age(now);
        if self.change_score > 9 && network_age < NETWORK_CHANGE_SECONDS {
            return false;
        }
        let required = u64::from(minutes) * 60;
        if network_age >= required {
            return true;
        }
        if since > 0
            && self.network_at <= since
            && (self.post_count() > 0 || self.report_count() > 5)
        {
            return true;
        }
        if self.password_age(now) < required {
            return false;
        }
        if since > 0 {
            return true;
        }
        (self.post_count() >= 3 || self.report_count() >= 10)
            && (network_age >= NETWORK_CHANGE_SECONDS
                || self.post_count() >= 9
                || self.report_count() >= 20)
    }

    pub fn is_known_or_verified(&self, now: u64, minutes: u32, since: u64) -> bool {
        self.verified_level > 0 || self.is_known(now, minutes, since)
    }

    pub fn update(&mut self, now: u64, activity: Activity, dummy: bool) {
        self.pending |= activity.bits();
        // idleLifetime has this absolute-timestamp fallback in the pinned
        // source when activity_ts is zero. Keep its activity decision intact.
        let idle = if self.activity_at > 0 {
            now.saturating_sub(self.activity_at)
        } else {
            self.created_at
        };
        let is_new = self.created_at == now;
        self.change_score = if idle < NETWORK_CHANGE_SECONDS && !is_new && self.network_at == now {
            self.change_score.saturating_add(3).min(32)
        } else if idle < NETWORK_CHANGE_SECONDS && !is_new && self.address_at == now {
            self.change_score.saturating_add(1).min(32)
        } else {
            self.change_score.saturating_sub(1)
        };
        if self.change_score >= 32 {
            self.posts = 0;
            self.images = 0;
            self.threads = 0;
            self.reports = 0;
            self.pending = 0;
        }
        if self.action_at == 0 {
            self.action_at = now;
        } else if !dummy && now.saturating_sub(self.action_at) >= ACTION_SECONDS {
            for (count, bit) in [
                (&mut self.posts, POST),
                (&mut self.images, IMAGE),
                (&mut self.threads, THREAD),
                (&mut self.reports, REPORT),
            ] {
                if self.pending & bit != 0 {
                    *count = count.saturating_add(1);
                }
            }
            self.pending = 0;
            self.action_at = now;
        }
        self.activity_at = now;
    }
}

fn age(now: u64, timestamp: u64) -> u64 {
    if timestamp > 0 {
        now.saturating_sub(timestamp)
    } else {
        0
    }
}
