//! Request-scoped D1 wrappers that record per-query metrics for `api_request` logs.

use serde::Deserialize;
use wasm_bindgen::JsCast;
use worker::d1::{D1PreparedStatement as InnerPreparedStatement, D1Result};
use worker::{D1Database as InnerD1Database, Result};

use crate::d1_metrics::{D1QueryIdentity, record_d1_query, record_d1_wall_clock};

#[derive(Debug, Clone)]
pub(crate) struct D1PreparedStatement {
    inner: InnerPreparedStatement,
    identity: D1QueryIdentity,
}

impl D1PreparedStatement {
    fn new(inner: InnerPreparedStatement, sql: &str) -> Self {
        Self {
            inner,
            identity: D1QueryIdentity::from_sql(sql, "prepare"),
        }
    }

    fn with_operation(&self, operation: &'static str) -> D1QueryIdentity {
        D1QueryIdentity {
            query_name: self.identity.query_name.clone(),
            statement_family: self.identity.statement_family.clone(),
            operation,
        }
    }

    pub fn bind_refs<'a, T, U>(&self, values: T) -> Result<Self>
    where
        T: IntoIterator<Item = &'a U>,
        U: worker::d1::D1Argument + 'a,
    {
        Ok(Self {
            inner: self.inner.bind_refs(values)?,
            identity: self.identity.clone(),
        })
    }

    pub async fn first<T>(&self, col_name: Option<&str>) -> Result<Option<T>>
    where
        T: for<'a> Deserialize<'a>,
    {
        let started_at_ms = js_sys::Date::now();
        let result = self.inner.first(col_name).await;
        record_d1_wall_clock(started_at_ms, &self.with_operation("first"));
        result
    }

    pub async fn run(&self) -> Result<D1Result> {
        let result = self.inner.run().await?;
        record_d1_result(&result, Some(&self.with_operation("run")));
        Ok(result)
    }

    pub async fn all(&self) -> Result<D1Result> {
        let result = self.inner.all().await?;
        record_d1_result(&result, Some(&self.with_operation("all")));
        Ok(result)
    }
}

#[derive(Debug)]
pub(crate) struct D1Database(InnerD1Database);

impl D1Database {
    pub(crate) fn new(inner: InnerD1Database) -> Self {
        Self(inner)
    }

    pub(crate) fn from_unchecked_js(value: impl Into<wasm_bindgen::JsValue>) -> Self {
        Self(InnerD1Database::unchecked_from_js(value.into()))
    }

    #[allow(dead_code)]
    pub(crate) fn inner(&self) -> &InnerD1Database {
        &self.0
    }

    pub fn prepare<T: Into<String>>(&self, query: T) -> D1PreparedStatement {
        let query = query.into();
        D1PreparedStatement::new(self.0.prepare(&query), &query)
    }

    pub async fn batch(&self, statements: Vec<D1PreparedStatement>) -> Result<Vec<D1Result>> {
        let identities = statements
            .iter()
            .map(|statement| statement.with_operation("batch"))
            .collect::<Vec<_>>();
        let statements = statements
            .into_iter()
            .map(|statement| statement.inner)
            .collect::<Vec<_>>();
        let results = self.0.batch(statements).await?;
        for (result, identity) in results.iter().zip(identities.iter()) {
            record_d1_result(result, Some(identity));
        }
        Ok(results)
    }

    #[allow(dead_code)]
    pub fn with_session(
        &self,
        constraint_or_bookmark: Option<&str>,
    ) -> Result<worker::D1DatabaseSession> {
        self.0.with_session(constraint_or_bookmark)
    }
}

fn record_d1_result(result: &D1Result, identity: Option<&D1QueryIdentity>) {
    let duration_ms = result
        .meta()
        .ok()
        .flatten()
        .and_then(|meta| meta.duration)
        .map(|duration| duration.max(0.0).round() as u64)
        .unwrap_or(0);
    record_d1_query(duration_ms, identity);
}
