// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::{Display, Formatter};

use serde_json::{Map, Value, json};

use super::model::{
    CURRENT_STANDARD_SCHEMA, StandardDiagnostic, StandardIntent, StandardMigration,
};

#[derive(Debug)]
pub enum StandardIntentDecodeError {
    InvalidRoot,
    UnsupportedSchema(u32),
    InvalidIntent(String),
}

impl Display for StandardIntentDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRoot => write!(f, "Standard Mode intent must be a JSON object"),
            Self::UnsupportedSchema(schema) => {
                write!(f, "unsupported Standard Mode schema {schema}")
            }
            Self::InvalidIntent(message) => write!(f, "invalid Standard Mode intent: {message}"),
        }
    }
}

impl std::error::Error for StandardIntentDecodeError {}

pub fn decode_standard_intent(
    value: Value,
) -> Result<(StandardIntent, Option<StandardMigration>), StandardIntentDecodeError> {
    let source = value
        .as_object()
        .ok_or(StandardIntentDecodeError::InvalidRoot)?;
    let schema = source
        .get("schema")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(1);

    let (value, migration) = match schema {
        CURRENT_STANDARD_SCHEMA => (value, None),
        5 => migrate_v5(value),
        4 => {
            let (value, v4_migration) = migrate_v4(value);
            let (value, v5_migration) = migrate_v5(value);
            (value, combine_migrations(4, [v4_migration, v5_migration]))
        }
        3 => {
            let (value, v3_migration) = migrate_v3(value);
            let (value, v4_migration) = migrate_v4(value);
            let (value, v5_migration) = migrate_v5(value);
            (
                value,
                combine_migrations(3, [v3_migration, v4_migration, v5_migration]),
            )
        }
        2 => {
            let (value, v2_migration) = migrate_v2(value);
            let (value, v3_migration) = migrate_v3(value);
            let (value, v4_migration) = migrate_v4(value);
            let (value, v5_migration) = migrate_v5(value);
            (
                value,
                combine_migrations(2, [v2_migration, v3_migration, v4_migration, v5_migration]),
            )
        }
        1 => {
            let (value, v1_migration) = migrate_v1(value);
            let (value, v2_migration) = migrate_v2(value);
            let (value, v3_migration) = migrate_v3(value);
            let (value, v4_migration) = migrate_v4(value);
            let (value, v5_migration) = migrate_v5(value);
            (
                value,
                combine_migrations(
                    1,
                    [
                        v1_migration,
                        v2_migration,
                        v3_migration,
                        v4_migration,
                        v5_migration,
                    ],
                ),
            )
        }
        other => return Err(StandardIntentDecodeError::UnsupportedSchema(other)),
    };

    serde_json::from_value(value)
        .map(|intent| (intent, migration))
        .map_err(|err| StandardIntentDecodeError::InvalidIntent(err.to_string()))
}

fn migrate_v5(mut value: Value) -> (Value, Option<StandardMigration>) {
    let root = value
        .as_object_mut()
        .expect("v5 migration input was checked as an object");
    root.insert("schema".to_string(), Value::from(CURRENT_STANDARD_SCHEMA));
    root.entry("dedicatedGroups".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    root.entry("dynamicLearning".to_string())
        .or_insert_with(|| json!({ "profiles": [] }));
    root.entry("advancedRules".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));

    let mut diagnostics = vec![StandardDiagnostic::warning(
        "schema_v5_migrated",
        "schema",
        "Standard Mode schema 5 was migrated with inactive Phase 3 defaults",
    )];
    if let Some(routing) = root.get_mut("routing").and_then(Value::as_object_mut)
        && let Some(scenarios) = routing.remove("scenarios")
        && scenarios
            .as_array()
            .is_some_and(|items| items.iter().any(legacy_scenario_enabled))
    {
        diagnostics.push(StandardDiagnostic::error(
            "legacy_scenario_requires_rebuild",
            "routing.scenarios",
            "an enabled legacy scenario placeholder cannot be guessed; rebuild it with a Phase 3 template",
        ));
    }

    (
        value,
        Some(StandardMigration {
            from_schema: 5,
            to_schema: CURRENT_STANDARD_SCHEMA,
            diagnostics,
        }),
    )
}

fn legacy_scenario_enabled(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|scenario| scenario.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn migrate_v4(mut value: Value) -> (Value, Option<StandardMigration>) {
    let root = value
        .as_object_mut()
        .expect("v4 migration input was checked as an object");
    root.insert("schema".to_string(), Value::from(5));
    root.entry("ruleData".to_string())
        .or_insert_with(|| json!({}));
    root.entry("smartRouting".to_string())
        .or_insert_with(|| json!({}));

    let mut diagnostics = vec![StandardDiagnostic::warning(
        "schema_v4_migrated",
        "schema",
        "Standard Mode schema 4 was migrated with inactive Phase 2 defaults",
    )];
    if let Some(paths) = root.get_mut("paths").and_then(Value::as_array_mut) {
        for (index, path) in paths.iter_mut().enumerate() {
            let Some(path) = path.as_object_mut() else {
                continue;
            };
            let legacy_ecs = path
                .remove("ecs")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "inherit".to_string());
            let ecs = match legacy_ecs.as_str() {
                "disabled" => json!({ "mode": "remove" }),
                "enabled" => {
                    diagnostics.push(StandardDiagnostic::warning(
                        "legacy_ecs_enabled_migrated",
                        format!("paths[{index}].ecs"),
                        "inactive legacy ECS enabled placeholder was migrated to client_subnet; review before Apply",
                    ));
                    json!({ "mode": "client_subnet", "mask4": 24, "mask6": 48 })
                }
                _ => json!({ "mode": "inherit" }),
            };
            path.insert("ecs".to_string(), ecs);

            let legacy_ip_selection = path
                .remove("ipSelection")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "inherit".to_string());
            if legacy_ip_selection == "enabled" {
                diagnostics.push(StandardDiagnostic::warning(
                    "legacy_ip_selection_enabled_migrated",
                    format!("paths[{index}].ipSelection"),
                    "inactive legacy IP selection placeholder was migrated to an enabled safe native policy; review before Apply",
                ));
            }
            path.insert(
                "ipSelection".to_string(),
                json!({ "enabled": legacy_ip_selection == "enabled" }),
            );
        }
    }

    (
        value,
        Some(StandardMigration {
            from_schema: 4,
            to_schema: 5,
            diagnostics,
        }),
    )
}

fn combine_migrations<const N: usize>(
    from_schema: u32,
    migrations: [Option<StandardMigration>; N],
) -> Option<StandardMigration> {
    let diagnostics = migrations
        .into_iter()
        .flatten()
        .flat_map(|migration| migration.diagnostics)
        .collect();
    Some(StandardMigration {
        from_schema,
        to_schema: CURRENT_STANDARD_SCHEMA,
        diagnostics,
    })
}

fn migrate_v3(mut value: Value) -> (Value, Option<StandardMigration>) {
    let root = value
        .as_object_mut()
        .expect("v3 migration input was checked as an object");
    root.insert("schema".to_string(), Value::from(4));
    root.entry("local".to_string()).or_insert_with(|| json!({}));
    if let Some(filtering) = root.get_mut("filtering").and_then(Value::as_object_mut) {
        filtering
            .entry("localFiles".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
    }
    (
        value,
        Some(StandardMigration {
            from_schema: 3,
            to_schema: 4,
            diagnostics: vec![StandardDiagnostic::warning(
                "schema_v3_migrated",
                "schema",
                "Standard Mode schema 3 was migrated with inactive Phase 1 defaults",
            )],
        }),
    )
}

fn migrate_v2(mut value: Value) -> (Value, Option<StandardMigration>) {
    let mut diagnostics = Vec::new();
    let root = value
        .as_object_mut()
        .expect("v2 migration input was checked as an object");
    root.insert("schema".to_string(), Value::from(3));

    if let Some(groups) = root.get_mut("upstreamGroups").and_then(Value::as_array_mut) {
        for (index, group) in groups.iter_mut().enumerate() {
            let Some(group) = group.as_object_mut() else {
                continue;
            };
            let strategy = group.get("strategy").and_then(Value::as_str);
            match strategy {
                Some("parallel") | None => {
                    group.insert("strategy".to_string(), Value::from("balanced"));
                }
                Some("sequential") => {
                    group.insert("strategy".to_string(), Value::from("balanced"));
                    diagnostics.push(StandardDiagnostic::warning(
                        "strategy_sequential_migrated",
                        format!("upstreamGroups[{index}].strategy"),
                        "legacy sequential did not implement ordered fallback and was migrated to balanced selection",
                    ));
                }
                Some("fastest") => {}
                Some(_) => {}
            }
        }
    }

    if let Some(cache) = root.get_mut("cache").and_then(Value::as_object_mut) {
        rename_key(cache, "minTtl", "minPositiveTtl");
        rename_key(cache, "maxTtl", "maxPositiveTtl");
        if let Some(negative_ttl) = cache.remove("negativeTtl") {
            cache
                .entry("maxNegativeTtl".to_string())
                .or_insert_with(|| negative_ttl.clone());
            cache
                .entry("negativeTtlWithoutSoa".to_string())
                .or_insert(negative_ttl);
        }
    }

    (
        value,
        Some(StandardMigration {
            from_schema: 2,
            to_schema: 3,
            diagnostics,
        }),
    )
}

fn migrate_v1(value: Value) -> (Value, Option<StandardMigration>) {
    let source = value.as_object().cloned().unwrap_or_default();
    let defaults = StandardIntent::default();
    let default_value = serde_json::to_value(&defaults).expect("default intent should serialize");
    let mut target = default_value.as_object().cloned().unwrap_or_default();

    if let Some(listen) = source.get("listen") {
        target.insert("listen".to_string(), listen.clone());
    }
    if let Some(cache) = source.get("cache") {
        target.insert("cache".to_string(), cache.clone());
    }
    if let Some(query_log) = source.get("queryLog") {
        target.insert("queryLog".to_string(), query_log.clone());
    }
    if let Some(system) = source.get("system") {
        target.insert("system".to_string(), system.clone());
    }

    let default_group = target
        .get("upstreamGroups")
        .and_then(Value::as_array)
        .and_then(|groups| groups.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut groups = vec![default_group];
    if let Some(upstreams) = source.get("upstreams").and_then(Value::as_array)
        && let Some(group) = groups.first_mut().and_then(Value::as_object_mut)
    {
        group.insert("upstreams".to_string(), upstreams.clone().into());
    }

    let mut paths = target
        .get("paths")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(domestic_upstreams) = source
        .get("split")
        .and_then(Value::as_object)
        .and_then(|split| split.get("domesticUpstreams"))
        .and_then(Value::as_array)
        && !domestic_upstreams.is_empty()
    {
        groups.push(json!({
            "id": "domestic",
            "name": "Domestic upstream group",
            "strategy": "parallel",
            "upstreams": domestic_upstreams,
        }));
        paths.push(json!({
            "id": "domestic",
            "name": "Domestic path",
            "upstreamGroupId": "domestic",
            "filtering": "inherit",
            "cache": "inherit",
            "queryLog": "inherit",
            "dualStack": "inherit",
            "ipSelection": "inherit",
            "ecs": "inherit",
        }));
    }
    target.insert("upstreamGroups".to_string(), Value::Array(groups));
    target.insert("paths".to_string(), Value::Array(paths));

    if let Some(ad_block) = source.get("adBlock").and_then(Value::as_object) {
        let mut filtering = target
            .get("filtering")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(enabled) = ad_block.get("enabled") {
            filtering.insert("enabled".to_string(), enabled.clone());
        }
        if let Some(rules) = ad_block.get("inlineRules") {
            filtering.insert("blockRules".to_string(), rules.clone());
        }
        target.insert("filtering".to_string(), Value::Object(filtering));
    }

    target.insert("schema".to_string(), Value::from(2));
    (
        Value::Object(target),
        Some(StandardMigration {
            from_schema: 1,
            to_schema: 2,
            diagnostics: vec![StandardDiagnostic::warning(
                "schema_v1_migrated",
                "schema",
                "legacy Standard Mode schema was migrated",
            )],
        }),
    )
}

fn rename_key(map: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = map.remove(from) {
        map.entry(to.to_string()).or_insert(value);
    }
}
