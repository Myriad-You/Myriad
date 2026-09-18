//! Schema check type definitions.

/// 列定义
#[derive(Debug, Clone, Default)]
pub(crate) struct ColumnDef {
    pub name: String,
    pub data_type: String,
    pub default_value: Option<String>,
    /// When true, generic ADD COLUMN repair emits `NOT NULL`.
    pub not_null: bool,
}

impl ColumnDef {
    pub(crate) fn new(name: impl Into<String>, data_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data_type: data_type.into(),
            default_value: None,
            not_null: false,
        }
    }

    pub(crate) fn not_null(mut self) -> Self {
        self.not_null = true;
        self
    }

    pub(crate) fn default_value(mut self, value: impl Into<String>) -> Self {
        self.default_value = Some(value.into());
        self
    }
}

/// 表定义
#[derive(Debug, Clone)]
pub(crate) struct TableDef {
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

/// 索引定义
#[derive(Debug, Clone)]
pub(crate) struct IndexDef {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
}
