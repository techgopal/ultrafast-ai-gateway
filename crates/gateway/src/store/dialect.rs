//! The one place that knows which database is behind the store.
//!
//! The store writes its SQL once, with `?` placeholders and nothing that only
//! SQLite understands; what still differs between databases is here, and every
//! statement goes through [`Dialect::sql`] on its way to the driver.

use std::borrow::Cow;

use sqlx::any::AnyArguments;
use sqlx::error::DatabaseError;
use sqlx::query::{Query, QueryAs, QueryScalar};
use sqlx::AnyConnection;
use sqlx::{Any, AssertSqlSafe};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    Sqlite,
    Postgres,
}

impl Dialect {
    /// The dialect a connection speaks.
    pub fn of(conn: &AnyConnection) -> Self {
        let name = conn.backend_name();
        if name.len() >= 8 && name.as_bytes()[..8].eq_ignore_ascii_case(b"postgres") {
            Self::Postgres
        } else {
            Self::Sqlite
        }
    }

    /// The dialect a database URL selects: `sqlite:` or `postgres(ql)://`.
    pub fn of_url(url: &str) -> Option<Self> {
        let scheme = url.split(':').next()?;
        match scheme {
            "sqlite" => Some(Self::Sqlite),
            "postgres" | "postgresql" => Some(Self::Postgres),
            _ => None,
        }
    }

    /// The statement as the driver takes it. SQLite keeps `?`; Postgres
    /// numbers them (`$1`, `$2`, ...) in order. A `?` inside a quoted
    /// literal or identifier is text, not a parameter.
    pub fn sql(self, q: &str) -> Cow<'_, str> {
        match self {
            Self::Sqlite => Cow::Borrowed(q),
            Self::Postgres => {
                if !q.contains('?') {
                    return Cow::Borrowed(q);
                }
                let mut out = String::with_capacity(q.len() + 8);
                let mut n = 0;
                let mut quote: Option<char> = None;
                for c in q.chars() {
                    match (quote, c) {
                        (Some(open), c) if c == open => {
                            // A doubled quote closes and opens again: still quoted.
                            quote = None;
                            out.push(c);
                        }
                        (Some(_), c) => out.push(c),
                        (None, '\'' | '"') => {
                            quote = Some(c);
                            out.push(c);
                        }
                        (None, '?') => {
                            n += 1;
                            out.push('$');
                            out.push_str(&n.to_string());
                        }
                        (None, c) => out.push(c),
                    }
                }
                Cow::Owned(out)
            }
        }
    }

    /// The text at `$.<name>` of a JSON object column, to compare or group
    /// by. It holds one `?` for the argument [`Dialect::tag_key`] makes.
    /// On PostgreSQL it sorts bytewise (`COLLATE "C"`), as SQLite does, so
    /// ties order the same on both.
    pub fn json_text(self, column: &str) -> String {
        match self {
            Self::Sqlite => format!("json_extract({column}, ?)"),
            Self::Postgres => format!("(({column}::jsonb ->> ?) COLLATE \"C\")"),
        }
    }

    /// The argument for [`Dialect::json_text`] that picks the top-level
    /// member `name`. Names are checked by the caller; SQLite quotes it so
    /// a `.`, `:` or `-` in one is part of the name.
    pub fn tag_key(self, name: &str) -> String {
        match self {
            Self::Sqlite => format!("$.\"{name}\""),
            Self::Postgres => name.to_string(),
        }
    }

    /// The larger of two values (SQLite's two-argument `MAX` is not
    /// an aggregate elsewhere).
    pub fn greatest(self, a: &str, b: &str) -> String {
        match self {
            Self::Sqlite => format!("MAX({a}, {b})"),
            Self::Postgres => format!("GREATEST({a}, {b})"),
        }
    }

    /// What orders the rows of a table that has no key column of its own
    /// (`route_grants`) the way they were inserted. On Postgres the table
    /// carries a `seq` identity column for it.
    pub fn insertion_order(self) -> &'static str {
        match self {
            Self::Sqlite => "rowid",
            Self::Postgres => "seq",
        }
    }

    /// Whether a failed write broke a unique constraint or index.
    pub fn is_unique_violation(e: &dyn DatabaseError) -> bool {
        e.is_unique_violation()
            || matches!(
                e.code().as_deref(),
                // SQLITE_CONSTRAINT_UNIQUE, SQLITE_CONSTRAINT_PRIMARYKEY, unique_violation
                Some("2067" | "1555" | "23505")
            )
    }

    /// Whether a failed write named a row that does not exist (a foreign key).
    pub fn is_foreign_key_violation(e: &dyn DatabaseError) -> bool {
        e.is_foreign_key_violation()
            || matches!(
                e.code().as_deref(),
                // SQLITE_CONSTRAINT_FOREIGNKEY, foreign_key_violation
                Some("787" | "23503")
            )
    }

    /// A statement written once for every database.
    pub fn query<'q>(self, sql: &'static str) -> Query<'q, Any, AnyArguments> {
        match self.sql(sql) {
            Cow::Borrowed(s) => sqlx::query(s),
            Cow::Owned(s) => sqlx::query(AssertSqlSafe(s)),
        }
    }

    /// Like [`Dialect::query`] for a statement put together at run time. The
    /// caller built it from constants; nothing a client sent is in it.
    pub fn query_dyn<'q>(self, sql: String) -> Query<'q, Any, AnyArguments> {
        sqlx::query(AssertSqlSafe(self.sql(&sql).into_owned()))
    }

    pub fn scalar<'q, O>(self, sql: &'static str) -> QueryScalar<'q, Any, O, AnyArguments>
    where
        (O,): for<'r> sqlx::FromRow<'r, sqlx::any::AnyRow>,
    {
        match self.sql(sql) {
            Cow::Borrowed(s) => sqlx::query_scalar(s),
            Cow::Owned(s) => sqlx::query_scalar(AssertSqlSafe(s)),
        }
    }

    pub fn scalar_dyn<'q, O>(self, sql: String) -> QueryScalar<'q, Any, O, AnyArguments>
    where
        (O,): for<'r> sqlx::FromRow<'r, sqlx::any::AnyRow>,
    {
        sqlx::query_scalar(AssertSqlSafe(self.sql(&sql).into_owned()))
    }

    pub fn query_as<'q, O>(self, sql: &'static str) -> QueryAs<'q, Any, O, AnyArguments>
    where
        O: for<'r> sqlx::FromRow<'r, sqlx::any::AnyRow>,
    {
        match self.sql(sql) {
            Cow::Borrowed(s) => sqlx::query_as(s),
            Cow::Owned(s) => sqlx::query_as(AssertSqlSafe(s)),
        }
    }
}

/// Anything that knows its dialect can start a statement, so a call site
/// reads `self.q("...")` or `conn.q("...")` and cannot forget the rewrite.
pub(crate) trait Dialected {
    fn dialect(&self) -> Dialect;

    fn q<'q>(&self, sql: &'static str) -> Query<'q, Any, AnyArguments> {
        self.dialect().query(sql)
    }

    fn q_dyn<'q>(&self, sql: String) -> Query<'q, Any, AnyArguments> {
        self.dialect().query_dyn(sql)
    }

    fn scalar<'q, O>(&self, sql: &'static str) -> QueryScalar<'q, Any, O, AnyArguments>
    where
        (O,): for<'r> sqlx::FromRow<'r, sqlx::any::AnyRow>,
    {
        self.dialect().scalar(sql)
    }

    fn scalar_dyn<'q, O>(&self, sql: String) -> QueryScalar<'q, Any, O, AnyArguments>
    where
        (O,): for<'r> sqlx::FromRow<'r, sqlx::any::AnyRow>,
    {
        self.dialect().scalar_dyn(sql)
    }

    fn query_as<'q, O>(&self, sql: &'static str) -> QueryAs<'q, Any, O, AnyArguments>
    where
        O: for<'r> sqlx::FromRow<'r, sqlx::any::AnyRow>,
    {
        self.dialect().query_as(sql)
    }
}

impl Dialected for AnyConnection {
    fn dialect(&self) -> Dialect {
        Dialect::of(self)
    }
}

impl Dialected for Dialect {
    fn dialect(&self) -> Dialect {
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_keeps_the_statement() {
        let q = "SELECT a FROM t WHERE b = ? AND c = ?";
        assert!(matches!(Dialect::Sqlite.sql(q), Cow::Borrowed(s) if s == q));
    }

    #[test]
    fn postgres_numbers_placeholders_in_order() {
        assert_eq!(
            Dialect::Postgres.sql("SELECT a FROM t WHERE b = ? AND (? IS NULL OR c < ?)"),
            "SELECT a FROM t WHERE b = $1 AND ($2 IS NULL OR c < $3)"
        );
    }

    #[test]
    fn a_question_mark_in_quotes_is_not_a_parameter() {
        assert_eq!(
            Dialect::Postgres.sql("SELECT '?' AS q, \"a?b\" FROM t WHERE x = ? AND y = 'it''s ?'"),
            "SELECT '?' AS q, \"a?b\" FROM t WHERE x = $1 AND y = 'it''s ?'"
        );
    }

    #[test]
    fn dialect_pieces() {
        assert_eq!(Dialect::Sqlite.greatest("a", "b"), "MAX(a, b)");
        assert_eq!(Dialect::Postgres.greatest("a", "b"), "GREATEST(a, b)");
        assert_eq!(
            Dialect::Sqlite.json_text("l.tags"),
            "json_extract(l.tags, ?)"
        );
        assert_eq!(
            Dialect::Postgres.json_text("l.tags"),
            "((l.tags::jsonb ->> ?) COLLATE \"C\")"
        );
        assert_eq!(Dialect::Sqlite.tag_key("a.b"), "$.\"a.b\"");
        assert_eq!(Dialect::Postgres.tag_key("a.b"), "a.b");
        assert_eq!(Dialect::Sqlite.insertion_order(), "rowid");
        assert_eq!(Dialect::of_url("sqlite://x.db"), Some(Dialect::Sqlite));
        assert_eq!(Dialect::of_url("postgres://u@h/d"), Some(Dialect::Postgres));
        assert_eq!(
            Dialect::of_url("postgresql://u@h/d"),
            Some(Dialect::Postgres)
        );
        assert_eq!(Dialect::of_url("mysql://u@h/d"), None);
    }

    /// The text of every double-quoted string literal in Rust source.
    fn string_literals(source: &str) -> Vec<String> {
        let chars: Vec<char> = source.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '/' if chars.get(i + 1) == Some(&'/') => {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                }
                // A char literal such as '"' or '\'': skip it whole.
                '\'' if chars.get(i + 2) == Some(&'\'') => i += 3,
                '\'' if chars.get(i + 1) == Some(&'\\') && chars.get(i + 3) == Some(&'\'') => {
                    i += 4
                }
                '"' => {
                    let mut text = String::new();
                    i += 1;
                    while i < chars.len() && chars[i] != '"' {
                        if chars[i] == '\\' {
                            i += 1;
                        }
                        if i < chars.len() {
                            text.push(chars[i]);
                        }
                        i += 1;
                    }
                    out.push(text);
                    i += 1;
                }
                _ => i += 1,
            }
        }
        out
    }

    /// No statement of the store may hold a `?` inside a quoted SQL literal:
    /// the rewrite leaves quoted text alone, so such a statement would mean
    /// a different thing on each database. This reads every string of the
    /// store's own source (above its unit tests) for a single-quoted literal
    /// that holds one.
    #[test]
    fn no_store_sql_has_a_question_mark_in_a_literal() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store");
        let mut statements = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("dialect.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let code = source.split("#[cfg(test)]").next().unwrap();
            for text in string_literals(code) {
                if !text.contains('?') {
                    continue;
                }
                statements += 1;
                let mut in_literal = false;
                for c in text.chars() {
                    match c {
                        '\'' => in_literal = !in_literal,
                        '?' if in_literal => {
                            panic!("{}: a '?' in a quoted literal: {text}", path.display())
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(
            statements > 100,
            "the scan found only {statements} statements"
        );
    }
}
