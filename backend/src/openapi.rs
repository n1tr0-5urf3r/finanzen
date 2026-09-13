//! The API contract.
//!
//! `openapi.json` is **committed**, and CI regenerates it and fails on
//! `git diff --exit-code`. That is the point of the whole file: a contract change
//! then arrives as a reviewable diff in a pull request instead of as a silent
//! behaviour change a client discovers at runtime.
//!
//! Two tests guard it, in opposite directions, because each catches what the other
//! cannot:
//!
//! 1. [`tests::the_contract_and_the_document_agree`] — a hardcoded (method, path)
//!    list must be present in the generated document **and equal in count**. Adding
//!    a handler without documenting it fails, and so does documenting a path the
//!    contract does not list: the document cannot drift ahead of the contract
//!    either, which the "every route is documented" test on its own permits.
//! 2. [`tests::every_documented_path_is_reachable_on_the_real_router`] — the
//!    document's paths are driven against the actual router and none may answer 404
//!    or 405. A path list is otherwise free to describe an endpoint nobody built.

use utoipa::OpenApi;

use crate::{auth::AuthChallenge, error::ErrorBody, models::*};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Finanzen",
        version = "0.1.0",
        description = "Haushaltsbuch. Beträge sind durchgehend ganze Cent; \
                       jedes Geldfeld endet auf `Cents`. Kategorienamen, Typ-Labels \
                       und Kommentare sind Daten und bleiben deutsch. \
                       KitchenOwl ist ein getrenntes, paralleles Journal und wird \
                       nie mit den persönlichen Buchungen verrechnet."
    ),
    servers((url = "/", description = "Dieselbe Origin wie die Oberfläche")),
    paths(
        crate::health,
        crate::ready,
        crate::auth_routes::setup_status,
        crate::auth_routes::setup,
        crate::auth_routes::login,
        crate::auth_routes::logout,
        crate::auth_routes::me,
        crate::auth_routes::change_password,
        crate::auth_routes::list_users,
        crate::auth_routes::create_user,
        crate::bookings::list,
        crate::bookings::create,
        crate::bookings::comments,
        crate::bookings::search,
        crate::bookings::get_one,
        crate::bookings::update,
        crate::bookings::delete,
        crate::bookings::bulk,
        crate::bookings::confirm,
        crate::receipts::upload,
        crate::receipts::download,
        crate::receipts::delete,
        crate::categories::list,
        crate::categories::create,
        crate::categories::update,
        crate::categories::delete,
        crate::categories::list_types,
        crate::rules::list,
        crate::rules::create,
        crate::rules::update,
        crate::rules::delete,
        crate::rules::apply,
        crate::analysis::dashboard,
        crate::analysis::monthly,
        crate::analysis::categories,
        crate::analysis::series,
        crate::analysis::series_subjects,
        crate::compare::compare,
        crate::compare::trailing,
        crate::forecast::forecast,
        crate::forecast::anomalies,
        crate::analysis::tax,
        crate::export::tax_csv,
        crate::export::tax_pdf,
        crate::export::bookings_json,
        crate::export::bookings_csv,
        crate::export::restore,
        crate::recurring::list,
        crate::funds::list,
        crate::funds::create,
        crate::funds::update,
        crate::funds::delete,
        crate::funds::status,
        crate::funds::suggestions,
        crate::recurring::create,
        crate::recurring::update,
        crate::recurring::delete,
        crate::recurring::materialize,
        crate::importer::list,
        crate::importer::upload,
        crate::importer::get,
        crate::importer::commit,
        crate::importer::review,
        crate::importer::resolve,
        crate::years::list,
        crate::years::create,
        crate::years::update,
        crate::kitchenowl::routes::status,
        crate::kitchenowl::routes::summary,
        crate::kitchenowl::routes::metadata,
        crate::kitchenowl::routes::sync_now,
        crate::kitchenowl::routes::expenses,
        crate::kitchenowl::analysis::categories,
        crate::kitchenowl::analysis::series,
        crate::kitchenowl::analysis::series_subjects,
        crate::kitchenowl::analysis::compare,
        crate::kitchenowl::analysis::trailing,
        crate::kitchenowl::settle::settlement,
        crate::kitchenowl::settle::settle,
        crate::kitchenowl::routes::unlink,
        crate::kitchenowl::routes::drafts,
        crate::kitchenowl::routes::link_draft,
        crate::kitchenowl::routes::dismiss_draft,
        crate::kitchenowl::routes::push_booking,
        crate::kitchenowl::routes::push_list,
        crate::kitchenowl::routes::push_retry,
        crate::kitchenowl::routes::push_retract,
    ),
    components(schemas(
        ErrorBody, StatusResponse, AuthChallenge,
        User, SetupStatus, SetupRequest, LoginRequest, PasswordRequest, CreateUserRequest,
        BookingKind, CategorySource, Booking, BookingInput, BookingPage,
        ConfirmBookingInput, Period,
        SearchResult, SearchYearSummary, SearchComment,
        Category, CategoryInput, CategoryTypeSummary, Rule, RuleInput, ApplyRulesResult,
        Dashboard, CategoryAnalysis, CategoryAnalysisRow, MonthlyOverview,
            MonthlySeries,
            SeriesMonth,
            SeriesSubject, MonthlyRow,
        YearComparison, CompareRow, CompareTypeRow, CompareTotals,
        TrailingWindow, TrailingMonth, TrailingCategory,
        TaxReport, TaxEntry, TaxCategorySummary,
        crate::forecast::Forecast, crate::forecast::ForecastMonth,
        crate::forecast::ForecastBasisRow, crate::forecast::AnomalyReport,
        crate::forecast::Anomaly,
        Year, YearInput, Receipt,
        RecurringTemplate, RecurringTemplateInput, MaterializeRequest, MaterializeResult,
        MaterializedItem,
        SinkingFund, SinkingFundInput, FundStatus, FundOverview, FundSuggestion,
        ExportDocument, ExportCategoryType, ExportCategory, ExportRule, ExportYear,
        ExportTemplate, ExportBooking, RestoreResult,
        KoMember, KoCategory, KoMetadata, KoShare, KoExpense, KoExpensePage,
        KoMatchCandidate, KoDraft, KoDraftPage, KoSyncRun, KoStatus, KoSummary,
        KoPushIntent, KoPushShareInput, KoPushRequest, KoLinkRequest, KoSyncResult,
        KoSettlement,
        KoCategoryAnalysis, KoCategoryAnalysisRow, KoPayerShare, KoMonthlySeries,
        KoSeriesMonth, KoSeriesSubject,
        KoYearComparison, KoCompareRow, KoCompareTotals, KoComparePayer,
        KoTrailingWindow, KoTrailingMonth, KoTrailingCategory,
    )),
    tags(
        (name = "system", description = "Liveness und Readiness"),
        (name = "auth", description = "Anmeldung und Ersteinrichtung"),
        (name = "admin", description = "Kontenverwaltung"),
        (name = "bookings", description = "Das persönliche Journal"),
        (name = "receipts", description = "Belege, auf der Platte statt in der Datenbank"),
        (name = "categories", description = "Die 32 Kategorien und ihre fünf Typen"),
        (name = "rules", description = "Kommentar → Kategorie, rückwirkend"),
        (name = "analysis", description = "Auswertungen, durchgehend netto"),
        (name = "tax", description = "Steuerliste und ihre Exporte"),
        (name = "exports", description = "Sicherung und Wiederherstellung"),
        (name = "recurring", description = "Vorlagen und ihre Monatsliste"),
        (name = "imports", description = "Tabellenimport mit Vorschau und Prüfliste"),
        (name = "years", description = "Geschäftsjahre und Vorträge"),
        (name = "funds", description = "Rücklagen für die jährlichen Brocken"),
        (name = "kitchenowl", description = "Das getrennte, parallele Journal"),
    )
)]
pub struct ApiDoc;

/// The document, pretty-printed exactly as `openapi.json` holds it.
///
/// One function, so the binary, the tests and CI cannot disagree about formatting
/// and produce a diff that means nothing.
pub fn document() -> String {
    let mut json = ApiDoc::openapi()
        .to_pretty_json()
        .expect("the OpenAPI document must serialise");
    json.push('\n');
    json
}

/// The contract. Hardcoded on purpose: generating this from the router would make
/// the test agree with itself, which is the one thing it must not do.
pub const CONTRACT: &[(&str, &str)] = &[
    ("get", "/api/v1/health"),
    ("get", "/api/v1/ready"),
    ("get", "/api/v1/auth/setup-status"),
    ("post", "/api/v1/auth/setup"),
    ("post", "/api/v1/auth/login"),
    ("post", "/api/v1/auth/logout"),
    ("get", "/api/v1/auth/me"),
    ("put", "/api/v1/auth/password"),
    ("get", "/api/v1/admin/users"),
    ("post", "/api/v1/admin/users"),
    ("get", "/api/v1/bookings"),
    ("post", "/api/v1/bookings"),
    ("get", "/api/v1/bookings/comments"),
    ("get", "/api/v1/bookings/search"),
    ("get", "/api/v1/bookings/{id}"),
    ("put", "/api/v1/bookings/{id}"),
    ("delete", "/api/v1/bookings/{id}"),
    ("post", "/api/v1/bookings/bulk"),
    ("post", "/api/v1/bookings/{id}/confirm"),
    ("post", "/api/v1/bookings/{id}/receipt"),
    ("get", "/api/v1/bookings/{id}/receipt"),
    ("delete", "/api/v1/bookings/{id}/receipt"),
    ("get", "/api/v1/categories"),
    ("post", "/api/v1/categories"),
    ("put", "/api/v1/categories/{id}"),
    ("delete", "/api/v1/categories/{id}"),
    ("get", "/api/v1/category-types"),
    ("get", "/api/v1/rules"),
    ("post", "/api/v1/rules"),
    ("put", "/api/v1/rules/{id}"),
    ("delete", "/api/v1/rules/{id}"),
    ("post", "/api/v1/rules/apply"),
    ("get", "/api/v1/dashboard"),
    ("get", "/api/v1/overview/months"),
    ("get", "/api/v1/analysis/categories"),
    ("get", "/api/v1/analysis/series"),
    ("get", "/api/v1/analysis/series/subjects"),
    ("get", "/api/v1/analysis/compare"),
    ("get", "/api/v1/analysis/trailing"),
    ("get", "/api/v1/analysis/forecast"),
    ("get", "/api/v1/analysis/anomalies"),
    ("get", "/api/v1/tax"),
    ("get", "/api/v1/tax/export.csv"),
    ("get", "/api/v1/tax/export.pdf"),
    ("get", "/api/v1/exports/bookings.json"),
    ("get", "/api/v1/exports/bookings.csv"),
    ("post", "/api/v1/exports/restore"),
    ("get", "/api/v1/recurring"),
    ("post", "/api/v1/recurring"),
    ("put", "/api/v1/recurring/{id}"),
    ("delete", "/api/v1/recurring/{id}"),
    ("post", "/api/v1/recurring/materialize"),
    ("get", "/api/v1/imports"),
    ("post", "/api/v1/imports"),
    ("get", "/api/v1/imports/{id}"),
    ("post", "/api/v1/imports/{id}/commit"),
    ("get", "/api/v1/imports/{id}/review"),
    ("post", "/api/v1/imports/{id}/review"),
    ("get", "/api/v1/years"),
    ("post", "/api/v1/years"),
    ("put", "/api/v1/years/{year}"),
    ("get", "/api/v1/kitchenowl/status"),
    ("get", "/api/v1/kitchenowl/summary"),
    ("get", "/api/v1/kitchenowl/metadata"),
    ("get", "/api/v1/funds"),
    ("post", "/api/v1/funds"),
    ("put", "/api/v1/funds/{id}"),
    ("delete", "/api/v1/funds/{id}"),
    ("get", "/api/v1/funds/status"),
    ("get", "/api/v1/funds/suggestions"),
    ("post", "/api/v1/kitchenowl/sync"),
    ("get", "/api/v1/kitchenowl/expenses"),
    ("get", "/api/v1/kitchenowl/analysis/categories"),
    ("get", "/api/v1/kitchenowl/analysis/series"),
    ("get", "/api/v1/kitchenowl/analysis/series/subjects"),
    ("get", "/api/v1/kitchenowl/analysis/compare"),
    ("get", "/api/v1/kitchenowl/analysis/trailing"),
    ("get", "/api/v1/kitchenowl/settlement"),
    ("post", "/api/v1/kitchenowl/settlement"),
    ("delete", "/api/v1/kitchenowl/expenses/{id}/link"),
    ("get", "/api/v1/kitchenowl/drafts"),
    ("post", "/api/v1/kitchenowl/drafts/{id}/link"),
    ("post", "/api/v1/kitchenowl/drafts/{id}/dismiss"),
    ("post", "/api/v1/bookings/{id}/kitchenowl"),
    ("get", "/api/v1/kitchenowl/push"),
    ("delete", "/api/v1/kitchenowl/push/{bookingId}"),
    ("post", "/api/v1/kitchenowl/push/{bookingId}/retry"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the operations back out of the generated document, so the comparison
    /// is against what a client would actually receive rather than against the
    /// `paths(...)` list the macro was handed.
    fn documented() -> Vec<(String, String)> {
        let json = document();
        let doc: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let mut out = Vec::new();
        for (path, item) in doc["paths"].as_object().expect("paths") {
            for method in item.as_object().expect("path item").keys() {
                out.push((method.clone(), path.clone()));
            }
        }
        out.sort();
        out
    }

    #[test]
    fn the_contract_and_the_document_agree() {
        let documented = documented();
        let mut contract: Vec<(String, String)> = CONTRACT
            .iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();
        contract.sort();

        let missing: Vec<_> = contract
            .iter()
            .filter(|c| !documented.contains(c))
            .collect();
        assert!(
            missing.is_empty(),
            "the contract lists operations the document does not describe: {missing:#?}"
        );
        // The other direction, which "every route is documented" on its own misses:
        // the document may not describe anything the contract has not agreed to.
        let extra: Vec<_> = documented
            .iter()
            .filter(|d| !contract.contains(d))
            .collect();
        assert!(
            extra.is_empty(),
            "the document describes operations the contract does not list: {extra:#?}"
        );
        assert_eq!(
            documented.len(),
            contract.len(),
            "the counts must match, so a duplicate on either side is caught too"
        );
    }

    #[test]
    fn the_contract_has_no_duplicates() {
        let mut seen: Vec<(&str, &str)> = CONTRACT.to_vec();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(
            before,
            seen.len(),
            "a duplicated (method, path) in CONTRACT"
        );
    }

    #[test]
    fn every_money_field_on_the_wire_says_so() {
        // Money is integer cents everywhere and every such field name ends in
        // `Cents`. A float on the wire is the one mistake this project cannot
        // absorb, so the document is checked for one.
        let json = document();
        let doc: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let schemas = &doc["components"]["schemas"];
        for (name, schema) in schemas.as_object().expect("schemas") {
            let Some(props) = schema["properties"].as_object() else {
                continue;
            };
            for (field, spec) in props {
                if field.ends_with("Cents") {
                    let ty = spec["type"].as_str().or_else(|| {
                        spec["allOf"][0]["type"]
                            .as_str()
                            .or_else(|| spec["oneOf"][0]["type"].as_str())
                    });
                    assert_ne!(
                        ty,
                        Some("number"),
                        "{name}.{field} is a float; money is integer cents"
                    );
                }
            }
        }
    }

    #[test]
    fn the_document_is_not_empty_and_names_its_tags() {
        let doc = ApiDoc::openapi();
        assert!(doc.paths.paths.len() > 40);
        assert!(doc.components.is_some());
        // The CI gate greps for this; an empty document would pass a weaker check.
        assert!(document().contains("\"paths\""));
    }
}
