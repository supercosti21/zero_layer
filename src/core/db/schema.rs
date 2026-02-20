use redb::TableDefinition;

pub const PACKAGES: TableDefinition<&str, &[u8]> = TableDefinition::new("packages");
pub const FILE_OWNERS: TableDefinition<&str, &str> = TableDefinition::new("file_owners");
pub const LIB_INDEX: TableDefinition<&str, &str> = TableDefinition::new("lib_index");
pub const DEPENDENCIES: TableDefinition<&str, &[u8]> = TableDefinition::new("dependencies");
pub const PLUGIN_META: TableDefinition<&str, &[u8]> = TableDefinition::new("plugin_meta");
pub const PINNED: TableDefinition<&str, &str> = TableDefinition::new("pinned");
