//! Adapts the data source registry to the assertion engine's [`DataSourceProvider`] implementation.
//!
//! The assertion layer depends only on the trait; this module decouples it from concrete connection management, keeping the assertion engine free of IO dependencies.

use async_trait::async_trait;
use orbit_assertion::{DataSourceProvider as _ProviderTrait, QueryResult};

use crate::registry::DataSourceRegistry;

#[async_trait]
impl _ProviderTrait for DataSourceRegistry {
    async fn query_sql(&self, datasource: &str, sql: &str) -> Result<QueryResult, String> {
        Self::query_sql(self, datasource, sql)
            .await
            .map_err(|e| e.message())
    }

    async fn redis_command(&self, datasource: &str, args: &[String]) -> Result<String, String> {
        Self::redis_cmd(self, datasource, args)
            .await
            .map_err(|e| e.message())
    }
}
