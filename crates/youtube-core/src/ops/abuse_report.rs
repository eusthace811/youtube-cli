//! Port of pkg/abuseReport and cmd/abuseReport.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::print_result;
use serde::Deserialize;
use youtube_api::Value;
use youtube_api::client::BoxFuture;
use youtube_api::generated::{ABUSE_REPORT, ABUSE_TYPE, RELATED_ENTITY};

const ERR_INSERT: &str = "failed to insert abuse report";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AbuseReport {
    #[serde(flatten)]
    pub fields: Fields,
    pub abuse_types: Vec<String>,
    pub description: String,
    pub subject_id: String,
    pub subject_type_id: String,
    pub subject_url: String,
    pub related_entity_id: String,
}

impl AbuseReport {
    pub async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut report = Value::new_struct(ABUSE_REPORT);
        if !self.abuse_types.is_empty() {
            let types = self
                .abuse_types
                .iter()
                .map(|id| {
                    let mut t = Value::new_struct(ABUSE_TYPE);
                    t.ensure("").set_str("id", id);
                    t
                })
                .collect();
            report.ensure("").set("abuseTypes", Value::List(types));
        }
        report.ensure("").set_str("description", &self.description);
        report
            .ensure("subject")
            .set_str("id", &self.subject_id)
            .set_str("typeId", &self.subject_type_id)
            .set_str("url", &self.subject_url);
        if !self.related_entity_id.is_empty() {
            let mut related = Value::new_struct(RELATED_ENTITY);
            related
                .ensure("entity")
                .set_str("id", &self.related_entity_id);
            report
                .ensure("")
                .set("relatedEntities", Value::List(vec![related]));
        }
        let res = svc
            .call("youtube.abuseReports.insert")
            .params("part", &self.fields.parts)
            .body(&report)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            "Abuse report submitted\n",
        )
        .map_err(OpError::new)
    }
}

impl Invoke for AbuseReport {
    fn confirm(&self) -> String {
        format!(
            "Create abuse report for {} {}",
            self.subject_type_id, self.subject_id
        )
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.insert(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(AbuseReport {
        fields: Fields {
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
        abuse_types: f.strings("abuseTypes"),
        description: f.string("description"),
        subject_id: f.string("subjectId"),
        subject_type_id: f.string("subjectTypeId"),
        subject_url: f.string("subjectUrl"),
        related_entity_id: f.string("relatedEntityId"),
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<AbuseReport>(args)
        .map(|r| Box::new(r) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "abuseReport insert",
    cli,
    mcp,
}];
