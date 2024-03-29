use anyhow::Context;

use tokio::{sync::OnceCell, time::Duration};

use tracing::debug;

use std::{fmt::Debug, time::SystemTime};

use crate::config::StaticFileCacheRuleType;

trait CacheRule: Send + Sync + Debug {
    fn matches(&self, resolved_path: &str) -> bool;

    fn build_cache_header(
        &self,
        resolved_file: &hyper_staticfile::ResolvedFile,
    ) -> Option<Duration>;
}

#[derive(Debug)]
struct FixedTimeCacheHeaderRule {
    path_regex: regex::Regex,
    file_cache_duration: Duration,
}

impl FixedTimeCacheHeaderRule {
    fn new(path_regex: regex::Regex, file_cache_duration: Duration) -> Self {
        Self {
            path_regex,
            file_cache_duration,
        }
    }
}

impl CacheRule for FixedTimeCacheHeaderRule {
    fn matches(&self, resolved_path: &str) -> bool {
        self.path_regex.is_match(resolved_path)
    }

    fn build_cache_header(&self, _: &hyper_staticfile::ResolvedFile) -> Option<Duration> {
        Some(self.file_cache_duration)
    }
}

#[derive(Debug)]
struct ModificationTimePlusDeltaCacheHeaderRule {
    path_regex: regex::Regex,
    file_cache_duration: Duration,
}

impl ModificationTimePlusDeltaCacheHeaderRule {
    fn new(path_regex: regex::Regex, file_cache_duration: Duration) -> Self {
        Self {
            path_regex,
            file_cache_duration,
        }
    }
}

impl CacheRule for ModificationTimePlusDeltaCacheHeaderRule {
    fn matches(&self, resolved_path: &str) -> bool {
        self.path_regex.is_match(resolved_path)
    }

    fn build_cache_header(
        &self,
        resolved_file: &hyper_staticfile::ResolvedFile,
    ) -> Option<Duration> {
        match resolved_file.modified {
            None => Some(Duration::from_secs(0)),
            Some(modified) => {
                let now = SystemTime::now();

                let file_expiration = modified + self.file_cache_duration;

                let request_cache_duration =
                    file_expiration.duration_since(now).unwrap_or_default();

                debug!(
                    "file_expiration = {:?} cache_duration = {:?}",
                    file_expiration, request_cache_duration
                );

                Some(request_cache_duration)
            }
        }
    }
}

#[derive(Debug)]
pub struct StaticFileRulesService {
    cache_rules: Vec<Box<dyn CacheRule>>,
}

impl StaticFileRulesService {
    fn new() -> anyhow::Result<Self> {
        let static_file_configuration = &crate::config::instance().static_file_configuration;

        let mut cache_rules: Vec<Box<dyn CacheRule>> =
            Vec::with_capacity(static_file_configuration.cache_rules.len());

        for cache_rule in &static_file_configuration.cache_rules {
            let path_regex = regex::Regex::new(&cache_rule.path_regex)
                .context("StaticFileRulesService::new: error parsing regex")?;

            match cache_rule.rule_type {
                StaticFileCacheRuleType::FixedTime => {
                    cache_rules.push(Box::new(FixedTimeCacheHeaderRule::new(
                        path_regex,
                        cache_rule.duration,
                    )));
                }
                StaticFileCacheRuleType::ModTimePlusDelta => {
                    cache_rules.push(Box::new(ModificationTimePlusDeltaCacheHeaderRule::new(
                        path_regex,
                        cache_rule.duration,
                    )));
                }
            }
        }

        debug!("cache_rules = {:?}", cache_rules,);

        Ok(Self { cache_rules })
    }

    pub fn build_cache_header(
        &self,
        resolved_file: &hyper_staticfile::ResolvedFile,
    ) -> Option<Duration> {
        let str_path = resolved_file.path.to_str().unwrap_or_default();

        self.cache_rules
            .iter()
            .find(|rule| rule.matches(str_path))
            .map(|rule| rule.build_cache_header(resolved_file))
            .unwrap_or(None)
    }
}

static RULES_SERVICE_INSTANCE: OnceCell<StaticFileRulesService> = OnceCell::const_new();

pub fn create_rules_service_instance() -> anyhow::Result<()> {
    let static_file_rules_service = StaticFileRulesService::new()?;

    RULES_SERVICE_INSTANCE
        .set(static_file_rules_service)
        .context("RULES_SERVICE_INSTANCE.set error")?;

    Ok(())
}

pub fn rules_service_instance() -> &'static StaticFileRulesService {
    RULES_SERVICE_INSTANCE.get().unwrap()
}
