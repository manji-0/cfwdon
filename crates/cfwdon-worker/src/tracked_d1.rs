//! Request-scoped D1 wrappers that record per-query metrics for `api_request` logs.

use serde::Deserialize;
use wasm_bindgen::JsCast;
use worker::d1::{D1PreparedStatement as InnerPreparedStatement, D1Result};
use worker::{D1Database as InnerD1Database, Result};

use crate::{record_d1_query_duration, record_d1_wall_clock};

#[derive(Debug, Clone)]
pub(crate) struct D1PreparedStatement(InnerPreparedStatement);

impl D1PreparedStatement {
    pub fn bind_refs<'a, T, U>(&self, values: T) -> Result<Self>
    where
        T: IntoIterator<Item = &'a U>,
        U: worker::d1::D1Argument + 'a,
    {
        Ok(Self(self.0.bind_refs(values)?))
    }

    pub async fn first<T>(&self, col_name: Option<&str>) -> Result<Option<T>>
    where
        T: for<'a> Deserialize<'a>,
    {
        let started_at_ms = js_sys::Date::now();
        let result = self.0.first(col_name).await;
        record_d1_wall_clock(started_at_ms);
        result
    }

    pub async fn run(&self) -> Result<D1Result> {
        let result = self.0.run().await?;
        record_d1_result(&result);
        Ok(result)
    }

    pub async fn all(&self) -> Result<D1Result> {
        let result = self.0.all().await?;
        record_d1_result(&result);
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
        D1PreparedStatement(self.0.prepare(query))
    }

    pub async fn batch(&self, statements: Vec<D1PreparedStatement>) -> Result<Vec<D1Result>> {
        let statements = statements
            .into_iter()
            .map(|statement| statement.0)
            .collect::<Vec<_>>();
        let results = self.0.batch(statements).await?;
        for result in &results {
            record_d1_result(result);
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

fn record_d1_result(result: &D1Result) {
    let duration_ms = result
        .meta()
        .ok()
        .flatten()
        .and_then(|meta| meta.duration)
        .map(|duration| duration.max(0.0).round() as u64)
        .unwrap_or(0);
    record_d1_query_duration(duration_ms);
}
