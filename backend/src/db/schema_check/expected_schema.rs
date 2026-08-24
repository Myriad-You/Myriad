//! Aggregated expected schema tables.
use super::types::TableDef;
use super::{
    tables_agent, tables_analytics, tables_brew, tables_core, tables_federation, tables_tapp,
};

pub(crate) fn get_expected_schema() -> Vec<TableDef> {
    let mut tables = tables_core::tables();
    tables.extend(tables_tapp::tables());
    tables.extend(tables_brew::tables());
    tables.extend(tables_agent::tables());
    tables.extend(tables_federation::tables());
    tables.extend(tables_analytics::tables());
    tables
}
