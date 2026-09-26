//! Aggregated expected schema tables.
use super::types::TableDef;
use super::{
    tables_agent, tables_analytics, tables_core, tables_federation, tables_local_music,
    tables_phantasi, tables_tapp,
};

pub(crate) fn get_expected_schema() -> Vec<TableDef> {
    let mut tables = tables_core::tables();
    tables.extend(tables_tapp::tables());
    tables.extend(tables_phantasi::tables());
    tables.extend(tables_agent::tables());
    tables.extend(tables_federation::tables());
    tables.extend(tables_analytics::tables());
    tables.extend(tables_local_music::tables());
    tables
}
