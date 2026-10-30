use std::{error::Error, sync::Arc};

use octo_core::{Connector, ConnectorFactory, ConnectorId, FactoryContext};
use toml::Value;

use super::{AdapterKind, AdapterRegistry};

/// Register only successfully constructed real adapters; connector descriptions
/// and model-provided names cannot create a physical delivery implementation.
pub struct ReportFactory {
    inner: Arc<dyn ConnectorFactory>,
    registry: AdapterRegistry,
    kind: AdapterKind,
}
impl ReportFactory {
    pub fn new(
        inner: Arc<dyn ConnectorFactory>,
        registry: AdapterRegistry,
        kind: AdapterKind,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner,
            registry,
            kind,
        })
    }
}
impl ConnectorFactory for ReportFactory {
    fn type_name(&self) -> &str {
        self.inner.type_name()
    }
    fn create(
        &self,
        id: ConnectorId,
        config: &Value,
        ctx: FactoryContext<'_>,
    ) -> Result<Arc<dyn Connector>, Box<dyn Error + Send + Sync>> {
        let connector = self.inner.create(id.clone(), config, ctx)?;
        self.registry.register(id.as_str(), self.kind);
        Ok(connector)
    }
}
