use crate::prelude::*;
use anylm::api::{Schema, Tool};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::fmt;

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new("handle_task", "Delegates a task using a specific skill.")
            .required_property(
                "skill",
                Schema::string("The existing skill required for this task."),
            )
            .required_property(
                "query",
                Schema::string("The detailed task prompt, parameters, and input context."),
            ),
    ]
}

/// The task action info
#[derive(Default, Debug, Clone, Serialize)]
pub struct TaskAction {
    #[serde(default)]
    pub tool_call_id: String,
    pub agent: String,
    pub skill: String,
    pub query: String,
}

impl TaskAction {
    /// Helper to get fully qualified skill name if needed ("agent_skill")
    pub fn full_skill(&self) -> String {
        format!("{}_{}", self.agent, self.skill)
    }
}

impl<'de> Deserialize<'de> for TaskAction {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum Field {
            ToolCallId,
            Skill,
            Query,
            Ignore,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("field identifier")
                    }

                    fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "tool_call_id" => Ok(Field::ToolCallId),
                            "skill" => Ok(Field::Skill),
                            "query" => Ok(Field::Query),
                            _ => Ok(Field::Ignore),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct TaskActionVisitor;

        impl<'de> Visitor<'de> for TaskActionVisitor {
            type Value = TaskAction;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct TaskAction")
            }

            fn visit_map<V>(self, mut map: V) -> StdResult<TaskAction, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut tool_call_id = None;
                let mut raw_skill: Option<String> = None;
                let mut query = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::ToolCallId => {
                            tool_call_id = Some(map.next_value()?);
                        }
                        Field::Skill => {
                            raw_skill = Some(map.next_value()?);
                        }
                        Field::Query => {
                            query = Some(map.next_value()?);
                        }
                        Field::Ignore => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }

                let raw_skill = raw_skill.ok_or_else(|| de::Error::missing_field("skill"))?;
                let query = query.ok_or_else(|| de::Error::missing_field("query"))?;

                // Разбиваем skill на agent и skill прямо при десериализации
                let (agent, skill) = raw_skill.split_once('_').ok_or_else(|| {
                    de::Error::custom(format!(
                        "invalid skill format '{raw_skill}': expected 'agent_skill'"
                    ))
                })?;

                Ok(TaskAction {
                    tool_call_id: tool_call_id.unwrap_or_default(),
                    agent: agent.to_string(),
                    skill: skill.to_string(),
                    query,
                })
            }
        }

        const FIELDS: &[&str] = &["tool_call_id", "skill", "query"];
        deserializer.deserialize_struct("TaskAction", FIELDS, TaskActionVisitor)
    }
}
