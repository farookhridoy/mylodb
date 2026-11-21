use serde::Serialize;

/// Which server software is behind a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Flavor {
    MySql,
    MariaDb,
}

/// One thing an engine may or may not do. When it cannot, `reason` says why in words a person can read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cap {
    pub available: bool,
    pub reason: Option<String>,
}

impl Cap {
    fn yes() -> Cap {
        Cap { available: true, reason: None }
    }
    fn no(reason: impl Into<String>) -> Cap {
        Cap { available: false, reason: Some(reason.into()) }
    }
}

/// What the connected server can do. The application layer and the UI ask for these, never for an engine name: a feature the engine
/// lacks is hidden or disabled with its reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub flavor: Flavor,
    /// `database.schema.table` (PostgreSQL, SQL Server). MySQL and MariaDB have databases only.
    pub schemas: Cap,
    pub routines: Cap,
    pub triggers: Cap,
    pub events: Cap,
    pub users: Cap,
    pub replication: Cap,
    /// `EXPLAIN ANALYZE` (MySQL 8.0.18+) or `ANALYZE` (MariaDB 10.1+).
    pub explain_analyze: Cap,
    /// A session the server itself keeps read-only.
    pub read_only_session: Cap,
    /// Row counts from table statistics instead of `COUNT(*)`.
    pub row_estimates: Cap,
}

/// The first numbers of a version string: `"8.0.36-0ubuntu1"` is `(8, 0, 36)`, `"10.11.6-MariaDB-1:10.11.6+maria~ubu2204"` is `(10, 11, 6)`.
fn numbers(version: &str) -> (u32, u32, u32) {
    let mut n = version.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u32>().unwrap_or(0));
    (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0))
}

impl Flavor {
    /// From the text of `SELECT VERSION()`.
    pub fn from_version(version: &str) -> Flavor {
        if version.to_lowercase().contains("mariadb") { Flavor::MariaDb } else { Flavor::MySql }
    }
}

impl Capabilities {
    /// For a MySQL or MariaDB server, from the text of `SELECT VERSION()`.
    pub fn mysql_family(version: &str) -> Capabilities {
        let flavor = Flavor::from_version(version);
        let v = numbers(version);
        let explain_analyze = match flavor {
            Flavor::MySql if v >= (8, 0, 18) => Cap::yes(),
            Flavor::MySql => Cap::no("EXPLAIN ANALYZE needs MySQL 8.0.18 or later"),
            Flavor::MariaDb if v >= (10, 1, 0) => Cap::yes(),
            Flavor::MariaDb => Cap::no("ANALYZE needs MariaDB 10.1 or later"),
        };
        Capabilities {
            flavor,
            schemas: Cap::no("MySQL and MariaDB have databases, not schemas inside a database"),
            routines: Cap::yes(),
            triggers: Cap::yes(),
            events: Cap::yes(),
            users: Cap::yes(),
            replication: Cap::yes(),
            explain_analyze,
            read_only_session: Cap::yes(),
            row_estimates: Cap::yes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flavor_comes_from_the_version_text() {
        assert_eq!(Flavor::from_version("9.7.2"), Flavor::MySql);
        assert_eq!(Flavor::from_version("8.0.36-0ubuntu0.22.04.1"), Flavor::MySql);
        assert_eq!(Flavor::from_version("10.11.6-MariaDB-1:10.11.6+maria~ubu2204"), Flavor::MariaDb);
        assert_eq!(Flavor::from_version("13.0.2-MariaDB"), Flavor::MariaDb);
    }

    #[test]
    fn explain_analyze_depends_on_the_server_and_its_version() {
        assert!(Capabilities::mysql_family("8.0.18").explain_analyze.available);
        assert!(Capabilities::mysql_family("9.7.2").explain_analyze.available);
        let old = Capabilities::mysql_family("5.7.44");
        assert!(!old.explain_analyze.available);
        assert!(old.explain_analyze.reason.unwrap().contains("8.0.18"));
        assert!(!Capabilities::mysql_family("8.0.17").explain_analyze.available);
        assert!(Capabilities::mysql_family("10.11.6-MariaDB").explain_analyze.available);
    }

    #[test]
    fn schemas_are_off_with_a_reason() {
        let c = Capabilities::mysql_family("9.7.2");
        assert!(!c.schemas.available && c.schemas.reason.is_some());
        assert!(c.events.available && c.users.available);
    }
}
