//! SPARQL 1.1 query fetcher — GET a query against a SPARQL endpoint's JSON
//! results and render the row bindings as plain text.
//!
//! Shared by `wikidata_myth` (`query.wikidata.org`) and `dbpedia_myth`
//! (`dbpedia.org`): both speak the same SPARQL 1.1 protocol and JSON results
//! shape (`{"head":{"vars":[...]},"results":{"bindings":[...]}}`), so one
//! fetcher parametrized by (endpoint, query) serves both rather than two
//! near-duplicate families. No new dependency: this is a plain GET through
//! the shared transport, decoded with `serde_json` (already a dependency).
//! Ports no Python precedent; the retired prototype declared neither source.

use std::fmt::Write as _;

use serde::Deserialize;

use crate::acquire::error::AcquireError;
use crate::acquire::fetch::FetchedWork;
use crate::acquire::http::{self, FetchContext};

/// The `wikidata_myth` source's SPARQL endpoint.
pub(crate) const WIKIDATA_ENDPOINT: &str = "https://query.wikidata.org/sparql";

/// The `wikidata_myth` source's query: every item that is an instance of (or
/// a transitive subclass-of instance of) `Q22988604` ("mythical character"),
/// with an English label and description via the label service. Verified
/// against the live endpoint when this fetcher was wired.
pub(crate) const WIKIDATA_MYTH_QUERY: &str = "SELECT ?item ?itemLabel ?itemDescription WHERE {
  ?item wdt:P31/wdt:P279* wd:Q22988604 .
  SERVICE wikibase:label { bd:serviceParam wikibase:language \"en\". }
} LIMIT 500";

/// The `dbpedia_myth` source's SPARQL endpoint.
pub(crate) const DBPEDIA_ENDPOINT: &str = "https://dbpedia.org/sparql";

/// The `dbpedia_myth` source's query: every resource typed
/// `dbo:Deity` or `dbo:MythologicalFigure` with an English `rdfs:label`, plus
/// its English `dbo:abstract` when present. Verified against the live
/// endpoint when this fetcher was wired.
pub(crate) const DBPEDIA_MYTH_QUERY: &str = "SELECT ?s ?label ?abstract WHERE {
  { ?s a <http://dbpedia.org/ontology/Deity> } UNION { ?s a <http://dbpedia.org/ontology/MythologicalFigure> }
  ?s rdfs:label ?label . FILTER(lang(?label) = 'en')
  OPTIONAL { ?s <http://dbpedia.org/ontology/abstract> ?abstract . FILTER(lang(?abstract) = 'en') }
} LIMIT 500";

/// The SPARQL 1.1 JSON results envelope, narrowed to what this fetcher
/// renders.
#[derive(Debug, Deserialize)]
struct SparqlResponse {
    head: Head,
    results: Results,
}

/// The declared result variables, in column order.
#[derive(Debug, Deserialize)]
struct Head {
    vars: Vec<String>,
}

/// The result rows: each a map from variable name to its bound value.
/// Variables an `OPTIONAL` clause left unbound are simply absent from the
/// row's map, per the SPARQL 1.1 JSON results spec.
#[derive(Debug, Deserialize)]
struct Results {
    bindings: Vec<std::collections::BTreeMap<String, Binding>>,
}

/// One bound value. `type`/`xml:lang`/`datatype` are not needed for a plain
/// text rendering, so only `value` is decoded.
#[derive(Debug, Deserialize)]
struct Binding {
    value: String,
}

/// Build the endpoint request URL for `query`, requesting the JSON results
/// format via the `format` query parameter — accepted by both `Wikidata`'s
/// and `DBpedia`'s SPARQL endpoints as a GET-friendly alternative to setting an
/// `Accept` header (which the shared [`http`] transport has no call-site hook
/// for today).
fn query_url(endpoint: &str, query: &str) -> String {
    format!("{endpoint}?query={}&format=json", urlencode(query))
}

/// Percent-encode a query string for use in a URL query parameter. SPARQL
/// queries in this pipeline are ASCII (keywords, IRIs, prefixes, string
/// literals), so a minimal reserved-character encoder is sufficient — no
/// full RFC 3986 percent-encoder dependency is pulled in for this.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// Render one SPARQL result set as plain text: for each row, one
/// `"<var>: <value>"` line per declared variable (in [`Head::vars`] order,
/// blank for a variable an `OPTIONAL` left unbound), with a blank line
/// between rows.
fn render(response: &SparqlResponse) -> String {
    let mut out = String::new();
    for row in &response.results.bindings {
        for var in &response.head.vars {
            let value = row.get(var).map_or("", |binding| binding.value.as_str());
            let _ = writeln!(out, "{var}: {value}");
        }
        out.push('\n');
    }
    out
}

/// Run `query` against `endpoint` and return the rendered result set as one
/// [`FetchedWork`].
///
/// # Errors
///
/// Returns [`AcquireError::Http`] if the request fails, or
/// [`AcquireError::Json`] if the response does not decode as the SPARQL 1.1
/// JSON results shape.
pub(crate) async fn fetch(
    ctx: &FetchContext<'_>,
    endpoint: &str,
    query: &str,
    title: Option<String>,
    retrieved: &str,
) -> Result<FetchedWork, AcquireError> {
    let url = query_url(endpoint, query);
    let bytes = http::get_bytes(ctx, &url).await?;
    let checksum = http::sha256_prefixed(&bytes);
    let response: SparqlResponse =
        serde_json::from_slice(&bytes).map_err(|source| AcquireError::Json {
            url: url.clone(),
            source,
        })?;

    Ok(FetchedWork {
        full_text: render(&response),
        url,
        checksum,
        retrieved: retrieved.to_owned(),
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_url_encodes_the_query_and_requests_json() {
        let url = query_url("https://query.wikidata.org/sparql", "SELECT ?x WHERE { }");
        assert_eq!(
            url,
            "https://query.wikidata.org/sparql?query=SELECT%20%3Fx%20WHERE%20%7B%20%7D&format=json"
        );
    }

    #[test]
    fn urlencode_leaves_unreserved_characters_untouched() {
        assert_eq!(urlencode("abc-XYZ_123.~"), "abc-XYZ_123.~");
    }

    #[test]
    fn urlencode_percent_encodes_reserved_characters() {
        assert_eq!(urlencode("?x { } <a/b>"), "%3Fx%20%7B%20%7D%20%3Ca%2Fb%3E");
    }

    #[test]
    fn render_writes_one_line_per_variable_per_row_with_a_blank_line_between_rows() {
        let response: SparqlResponse = serde_json::from_str(
            r#"{
                "head": {"vars": ["item", "itemLabel"]},
                "results": {"bindings": [
                    {"item": {"type": "uri", "value": "http://www.wikidata.org/entity/Q1"},
                     "itemLabel": {"type": "literal", "value": "Zeus"}},
                    {"item": {"type": "uri", "value": "http://www.wikidata.org/entity/Q2"},
                     "itemLabel": {"type": "literal", "value": "Hera"}}
                ]}
            }"#,
        )
        .expect("fixture parses as the sparql results shape");

        assert_eq!(
            render(&response),
            "item: http://www.wikidata.org/entity/Q1\nitemLabel: Zeus\n\n\
             item: http://www.wikidata.org/entity/Q2\nitemLabel: Hera\n\n"
        );
    }

    #[test]
    fn render_leaves_an_unbound_optional_variable_blank() {
        let response: SparqlResponse = serde_json::from_str(
            r#"{
                "head": {"vars": ["s", "label", "abstract"]},
                "results": {"bindings": [
                    {"s": {"type": "uri", "value": "http://dbpedia.org/resource/Juventas"},
                     "label": {"type": "literal", "value": "Juventas"}}
                ]}
            }"#,
        )
        .expect("fixture parses with an unbound optional variable");

        assert_eq!(
            render(&response),
            "s: http://dbpedia.org/resource/Juventas\nlabel: Juventas\nabstract: \n\n"
        );
    }

    #[test]
    fn render_of_an_empty_result_set_is_empty() {
        let response: SparqlResponse =
            serde_json::from_str(r#"{"head": {"vars": ["s"]}, "results": {"bindings": []}}"#)
                .expect("fixture parses as an empty result set");
        assert_eq!(render(&response), "");
    }
}
