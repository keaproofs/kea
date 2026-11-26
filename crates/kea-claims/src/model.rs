//! Which Claude model drafts, and what its tokens cost.

/// A drafting model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// Claude Opus 5.5: the strongest drafter.
    Opus,
    /// Claude Sonnet 5.5.
    Sonnet,
    /// Claude Haiku 5.5: the cheapest.
    Haiku,
}

/// Prices in US dollars per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prices {
    /// Uncached input.
    pub input: f64,
    /// Input written to the cache (five-minute lifetime): 1.25 times input.
    pub cache_write: f64,
    /// Input read from the cache.
    pub cache_read: f64,
    /// Output, thinking included.
    pub output: f64,
}

impl Model {
    /// The API model id.
    pub fn id(self) -> &'static str {
        match self {
            Self::Opus => "claude-opus-5-5",
            Self::Sonnet => "claude-sonnet-5-5",
            Self::Haiku => "claude-haiku-5-5",
        }
    }

    /// The model named by `name`: an API id or the family word.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "opus" | "claude-opus-5-5" => Some(Self::Opus),
            "sonnet" | "claude-sonnet-5-5" => Some(Self::Sonnet),
            "haiku" | "claude-haiku-5-5" => Some(Self::Haiku),
            _ => None,
        }
    }

    /// List prices, as published in November 2026. Known limit: these are written down, not read
    /// from the API, so a price change shows up only as a wrong cost estimate.
    pub fn prices(self) -> Prices {
        let (input, cache_read, output) = match self {
            Self::Opus => (4.0, 0.20, 20.0),
            Self::Sonnet => (2.0, 0.10, 10.0),
            Self::Haiku => (0.10, 0.01, 0.50),
        };
        Prices { input, cache_write: input * 1.25, cache_read, output }
    }

    /// Whether the API can re-run a declined request on another model inside the same call.
    /// Contract analysis sits close to what safety classifiers watch, so a decline is possible.
    pub fn has_server_fallback(self) -> bool {
        matches!(self, Self::Opus | Self::Sonnet)
    }
}

/// Token counts of one response.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Uncached input tokens.
    pub input: u64,
    /// Input tokens written to the cache.
    pub cache_write: u64,
    /// Input tokens read from the cache.
    pub cache_read: u64,
    /// Output tokens, thinking included.
    pub output: u64,
}

impl Usage {
    /// What these tokens cost on `model`, in US dollars.
    #[allow(clippy::cast_precision_loss)] // token counts stay far below 2^52
    pub fn cost(&self, model: Model) -> f64 {
        let p = model.prices();
        (self.input as f64 * p.input
            + self.cache_write as f64 * p.cache_write
            + self.cache_read as f64 * p.cache_read
            + self.output as f64 * p.output)
            / 1_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_follow_the_price_list() {
        let usage = Usage { input: 1_000_000, cache_write: 0, cache_read: 1_000_000, output: 100_000 };
        assert!((usage.cost(Model::Opus) - (4.0 + 0.20 + 2.0)).abs() < 1e-9);
        assert!((usage.cost(Model::Haiku) - (0.10 + 0.01 + 0.05)).abs() < 1e-9);
        assert_eq!(Model::parse("sonnet"), Some(Model::Sonnet));
        assert_eq!(Model::parse(Model::Opus.id()), Some(Model::Opus));
        assert!(!Model::Haiku.has_server_fallback());
    }
}
