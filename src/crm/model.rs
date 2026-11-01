//! Companies, people, notes, and the passages a summary is allowed to use.
//!
//! A highlight is a span in a source, not a paraphrase. Proposals and activity
//! point at those spans. Import skips a company whose domain is already here.

use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Id(u64);

impl Id {
    pub(crate) fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    Lead,
    Qualified,
    Pilot,
    Customer,
}

impl Stage {
    pub(crate) const ALL: [Stage; 4] = [Self::Lead, Self::Qualified, Self::Pilot, Self::Customer];

    pub(crate) fn label(self) -> &'static str {
        ["Lead", "Qualified", "Pilot", "Customer"][self as usize]
    }

    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "lead" => Some(Self::Lead),
            "qualified" => Some(Self::Qualified),
            "pilot" => Some(Self::Pilot),
            "customer" => Some(Self::Customer),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Object {
    Companies,
    People,
    Tasks,
}

impl Object {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Companies => "Companies",
            Self::People => "People",
            Self::Tasks => "Tasks",
        }
    }
}

/// A saved projection of the company list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum View {
    All,
    NeedsFollowUp,
}

impl View {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NeedsFollowUp => "Needs follow-up",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Company {
    id: Id,
    name: String,
    domain: String,
    stage: Stage,
    owner: String,
    employees: u32,
    last_interaction: String,
    next_interaction: String,
}

impl Company {
    pub(crate) fn id(&self) -> Id {
        self.id
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn domain(&self) -> &str {
        &self.domain
    }
    pub(crate) fn stage(&self) -> Stage {
        self.stage
    }
    pub(crate) fn owner(&self) -> &str {
        &self.owner
    }
    pub(crate) fn employees(&self) -> u32 {
        self.employees
    }
    pub(crate) fn last_interaction(&self) -> &str {
        &self.last_interaction
    }
    pub(crate) fn next_interaction(&self) -> &str {
        &self.next_interaction
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Person {
    id: Id,
    name: String,
    email: String,
    title: String,
    company: Id,
}

impl Person {
    pub(crate) fn id(&self) -> Id {
        self.id
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn email(&self) -> &str {
        &self.email
    }
    pub(crate) fn title(&self) -> &str {
        &self.title
    }
    pub(crate) fn company(&self) -> Id {
        self.company
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Note {
    body: String,
}

impl Note {
    pub(crate) fn body(&self) -> &str {
        &self.body
    }
}

/// How a source was read. The passages stay the evidence, whatever the file was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaterialKind {
    Text,
    Json,
    Table,
    Database,
}

impl MaterialKind {
    pub(crate) fn as_str(self) -> &'static str {
        ["text", "json", "table", "database"][self as usize]
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            "table" => Some(Self::Table),
            "database" => Some(Self::Database),
            _ => None,
        }
    }

    pub(crate) fn caption(self) -> &'static str {
        const CAPTIONS: [&str; 4] = [
            "Text. A highlight keeps the paragraph it came from.",
            "JSON object. A highlight keeps the field it came from.",
            "Table. A highlight keeps the row it came from.",
            "Database. A highlight keeps the row it came from.",
        ];
        CAPTIONS[self as usize]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Source {
    id: Id,
    company: Id,
    title: String,
    paragraphs: Vec<String>,
    kind: MaterialKind,
    /// File name under the CRM files directory. The original stays where it was.
    file: Option<String>,
}

impl Source {
    pub(crate) fn id(&self) -> Id {
        self.id
    }
    pub(crate) fn title(&self) -> &str {
        &self.title
    }
    pub(crate) fn paragraphs(&self) -> &[String] {
        &self.paragraphs
    }
    pub(crate) fn kind(&self) -> MaterialKind {
        self.kind
    }
    pub(crate) fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }
    pub(crate) fn company(&self) -> Id {
        self.company
    }
}

/// A span in a source. Themes hang off the span; they do not replace it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Evidence {
    id: Id,
    source: Id,
    company: Id,
    paragraph: usize,
    text: String,
    themes: Vec<String>,
}

impl Evidence {
    pub(crate) fn id(&self) -> Id {
        self.id
    }
    pub(crate) fn company(&self) -> Id {
        self.company
    }
    pub(crate) fn source(&self) -> Id {
        self.source
    }
    pub(crate) fn paragraph(&self) -> usize {
        self.paragraph
    }
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn themes(&self) -> &[String] {
        &self.themes
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Task {
    id: Id,
    company: Id,
    title: String,
    evidence: Option<Id>,
}

impl Task {
    pub(crate) fn id(&self) -> Id {
        self.id
    }
    pub(crate) fn company(&self) -> Id {
        self.company
    }
    pub(crate) fn title(&self) -> &str {
        &self.title
    }
    pub(crate) fn evidence(&self) -> Option<Id> {
        self.evidence
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Insight {
    title: String,
    body: String,
    evidence: Vec<Id>,
}

impl Insight {
    pub(crate) fn title(&self) -> &str {
        &self.title
    }
    pub(crate) fn body(&self) -> &str {
        &self.body
    }
    pub(crate) fn evidence(&self) -> &[Id] {
        &self.evidence
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Activity {
    summary: String,
    detail: String,
    source: Option<String>,
}

impl Activity {
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }
    pub(crate) fn detail(&self) -> &str {
        &self.detail
    }
    pub(crate) fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Proposal {
    pub(crate) summary: String,
    pub(crate) stage: Option<Stage>,
    pub(crate) next_interaction: Option<String>,
    pub(crate) task: Option<String>,
    pub(crate) insight_title: String,
    pub(crate) insight_body: String,
    pub(crate) evidence: Vec<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AskError {
    /// The company has a source and no tagged span.
    NoEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportReport {
    companies_added: usize,
    people_added: usize,
    duplicates: Vec<String>,
}

impl ImportReport {
    pub(crate) fn companies_added(&self) -> usize {
        self.companies_added
    }
    pub(crate) fn people_added(&self) -> usize {
        self.people_added
    }
    pub(crate) fn duplicates(&self) -> &[String] {
        &self.duplicates
    }
}

#[derive(Clone)]
struct Snapshot {
    next: u64,
    companies: Vec<Company>,
    people: Vec<Person>,
    notes: Vec<(Id, Note)>,
    sources: Vec<Source>,
    evidence: Vec<Evidence>,
    tasks: Vec<Task>,
    insights: Vec<(Id, Insight)>,
    activity: Vec<(Id, Activity)>,
}

pub(crate) struct Store {
    next: u64,
    companies: Vec<Company>,
    people: Vec<Person>,
    notes: Vec<(Id, Note)>,
    sources: Vec<Source>,
    evidence: Vec<Evidence>,
    tasks: Vec<Task>,
    insights: Vec<(Id, Insight)>,
    activity: Vec<(Id, Activity)>,
    report: ImportReport,
    undo: Vec<Snapshot>,
}

const COMPANIES: &str = "\
name,domain,stage,owner,employees,last,next
Acme Robotics,acme.example,Lead,Mina Cole,120,3 days ago,—
Northwind Goods,northwind.example,Qualified,Samir Adeyemi,40,1 week ago,Send the catalog
Helio Health,helio.example,Customer,Mina Cole,800,Yesterday,Renewal in March
Keel Studio,keel.example,Lead,Ruth Pell,12,2 months ago,—
Acme Robotics,acme.example,Lead,Mina Cole,120,3 days ago,—
";

const PEOPLE: &str = "\
name,email,title,company
Ada Chen,ada@acme.example,Chief Executive,Acme Robotics
Jules Okonkwo,jules@acme.example,Operations,Acme Robotics
Priya Raman,priya@northwind.example,Buyer,Northwind Goods
Owen Blake,owen@helio.example,Procurement,Helio Health
Leah Cho,leah@helio.example,Finance,Helio Health
Noor Elmasri,noor@keel.example,Founder,Keel Studio
";

impl Store {
    pub(crate) fn sample() -> Self {
        let mut store = Self::import(COMPANIES, PEOPLE);
        store.attach_acme_interview();
        store.undo.clear();
        store
    }

    pub(crate) fn import(companies: &str, people: &str) -> Self {
        let mut store = Self {
            next: 1,
            companies: Vec::new(),
            people: Vec::new(),
            notes: Vec::new(),
            sources: Vec::new(),
            evidence: Vec::new(),
            tasks: Vec::new(),
            insights: Vec::new(),
            activity: Vec::new(),
            report: ImportReport {
                companies_added: 0,
                people_added: 0,
                duplicates: Vec::new(),
            },
            undo: Vec::new(),
        };
        for row in records(companies) {
            if row.len() < 7 {
                continue;
            }
            let domain = row[1].to_ascii_lowercase();
            if store
                .companies
                .iter()
                .any(|company| company.domain.eq_ignore_ascii_case(&domain))
            {
                store.report.duplicates.push(domain);
                continue;
            }
            let Some(stage) = Stage::parse(&row[2]) else {
                continue;
            };
            let id = store.fresh();
            store.companies.push(Company {
                id,
                name: row[0].clone(),
                domain,
                stage,
                owner: row[3].clone(),
                employees: row[4].parse().unwrap_or(0),
                last_interaction: row[5].clone(),
                next_interaction: row[6].clone(),
            });
            store.report.companies_added += 1;
        }
        for row in records(people) {
            if row.len() < 4 {
                continue;
            }
            let Some(company) = store
                .companies
                .iter()
                .find(|company| company.name.eq_ignore_ascii_case(&row[3]))
                .map(|company| company.id)
            else {
                continue;
            };
            let id = store.fresh();
            store.people.push(Person {
                id,
                name: row[0].clone(),
                email: row[1].clone(),
                title: row[2].clone(),
                company,
            });
            store.report.people_added += 1;
        }
        let added = store.report.companies_added;
        let people_added = store.report.people_added;
        let skipped = store.report.duplicates.len();
        store.push_activity(
            store
                .companies
                .first()
                .map(|company| company.id)
                .unwrap_or(Id(0)),
            format!("Imported {added} companies and {people_added} people"),
            if skipped == 0 {
                "No duplicate domains.".into()
            } else {
                format!(
                    "Skipped {skipped} duplicate {}.",
                    if skipped == 1 { "domain" } else { "domains" }
                )
            },
            None,
        );
        // The import receipt belongs to the workspace, not one company. Copy
        // it onto every company so each record can show where it came from.
        if let Some(first) = store.activity.first().cloned() {
            let ids: Vec<Id> = store
                .companies
                .iter()
                .skip(1)
                .map(|company| company.id)
                .collect();
            for id in ids {
                store.activity.push((id, first.1.clone()));
            }
        }
        store
    }

    pub(crate) fn report(&self) -> &ImportReport {
        &self.report
    }

    pub(crate) fn companies(&self, view: View) -> Vec<&Company> {
        self.companies
            .iter()
            .filter(|company| match view {
                View::All => true,
                View::NeedsFollowUp => needs_follow_up(&company.next_interaction),
            })
            .collect()
    }

    pub(crate) fn company(&self, id: Id) -> Option<&Company> {
        self.companies.iter().find(|company| company.id == id)
    }

    pub(crate) fn people(&self) -> &[Person] {
        &self.people
    }

    pub(crate) fn person(&self, id: Id) -> Option<&Person> {
        self.people.iter().find(|person| person.id == id)
    }

    pub(crate) fn people_at(&self, company: Id) -> Vec<&Person> {
        rows_for(&self.people, company, Person::company)
    }

    pub(crate) fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    pub(crate) fn tasks_at(&self, company: Id) -> Vec<&Task> {
        rows_for(&self.tasks, company, Task::company)
    }

    pub(crate) fn notes_at(&self, company: Id) -> Vec<&Note> {
        owned(&self.notes, company)
    }

    pub(crate) fn sources_at(&self, company: Id) -> Vec<&Source> {
        rows_for(&self.sources, company, Source::company)
    }

    pub(crate) fn source(&self, id: Id) -> Option<&Source> {
        self.sources.iter().find(|source| source.id == id)
    }

    pub(crate) fn evidence(&self) -> &[Evidence] {
        &self.evidence
    }

    pub(crate) fn evidence_by_id(&self, id: Id) -> Option<&Evidence> {
        self.evidence.iter().find(|evidence| evidence.id == id)
    }

    pub(crate) fn evidence_at(&self, company: Id) -> Vec<&Evidence> {
        rows_for(&self.evidence, company, Evidence::company)
    }

    pub(crate) fn evidence_for(&self, source: Id, paragraph: usize) -> Option<&Evidence> {
        self.evidence
            .iter()
            .find(|evidence| evidence.source == source && evidence.paragraph == paragraph)
    }

    pub(crate) fn insights_at(&self, company: Id) -> Vec<&Insight> {
        owned(&self.insights, company)
    }

    pub(crate) fn activity_at(&self, company: Id) -> Vec<&Activity> {
        let mut found = owned(&self.activity, company);
        found.reverse();
        found
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.next = snapshot.next;
        self.companies = snapshot.companies;
        self.people = snapshot.people;
        self.notes = snapshot.notes;
        self.sources = snapshot.sources;
        self.evidence = snapshot.evidence;
        self.tasks = snapshot.tasks;
        self.insights = snapshot.insights;
        self.activity = snapshot.activity;
        true
    }

    pub(crate) fn set_stage(&mut self, company: Id, stage: Stage) {
        let Some(current) = self.company(company).map(|company| company.stage) else {
            return;
        };
        if current == stage {
            return;
        }
        self.remember();
        if let Some(found) = self.companies.iter_mut().find(|item| item.id == company) {
            found.stage = stage;
        }
        self.push_activity(
            company,
            format!("Stage set to {}", stage.label()),
            format!("{} → {}", current.label(), stage.label()),
            None,
        );
    }

    /// Keep a file's passages on the company. The file name is the copy's name.
    pub(crate) fn attach(
        &mut self,
        company: Id,
        title: String,
        kind: MaterialKind,
        passages: Vec<String>,
        file: Option<String>,
    ) -> Option<Id> {
        if self.company(company).is_none() || passages.is_empty() {
            return None;
        }
        self.remember();
        let id = self.fresh();
        let count = passages.len();
        self.sources.push(Source {
            id,
            company,
            title: title.clone(),
            paragraphs: passages,
            kind,
            file,
        });
        self.push_activity(
            company,
            format!("Attached {title}"),
            format!(
                "{count} {}",
                if count == 1 { "passage" } else { "passages" }
            ),
            Some(title),
        );
        Some(id)
    }

    pub(crate) fn set_file(&mut self, source: Id, file: String) {
        if let Some(found) = self.sources.iter_mut().find(|item| item.id == source) {
            found.file = Some(file);
        }
    }

    /// Write the records. The undo stack stays in this session.
    pub(crate) fn save(&self, dir: &Path) {
        let path = dir.join("crm.json");
        if let Err(error) = fs::create_dir_all(dir) {
            log::warn!("creating {}: {error}", dir.display());
            return;
        }
        let temporary = path.with_extension("json.tmp");
        let result = serde_json::to_vec_pretty(&self.to_json())
            .map_err(std::io::Error::other)
            .and_then(|bytes| fs::write(&temporary, bytes))
            .and_then(|()| fs::rename(&temporary, &path));
        if let Err(error) = result {
            log::warn!("writing {}: {error}", path.display());
        }
    }

    /// The records in `dir`, or nothing when the file is missing or unreadable.
    /// An unreadable file is moved aside so the next save does not erase it.
    pub(crate) fn load(dir: &Path) -> Option<Self> {
        let path = dir.join("crm.json");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return None,
            Err(error) => {
                log::warn!("reading {}: {error}", path.display());
                return None;
            }
        };
        match serde_json::from_str(&text)
            .ok()
            .and_then(|value| Self::from_json(&value))
        {
            Some(store) => Some(store),
            None => {
                let aside = path.with_extension("json.unreadable");
                log::warn!(
                    "{} is not a CRM file; moved to {}",
                    path.display(),
                    aside.display()
                );
                let _ = fs::rename(&path, aside);
                None
            }
        }
    }

    pub(crate) fn add_note(&mut self, company: Id, body: String) {
        let body = body.trim().to_owned();
        if body.is_empty() || self.company(company).is_none() {
            return;
        }
        self.remember();
        self.notes.push((company, Note { body: body.clone() }));
        self.push_activity(company, "Note added".into(), body, None);
    }

    /// Tag `paragraph` of `source`. A second tag on the same span is another
    /// theme, not another piece of evidence.
    pub(crate) fn tag(&mut self, source: Id, paragraph: usize, theme: &str) -> Option<Id> {
        let theme = theme.trim();
        if theme.is_empty() {
            return None;
        }
        let Some(found) = self.source(source) else {
            return None;
        };
        let Some(text) = found.paragraphs.get(paragraph).cloned() else {
            return None;
        };
        let company = found.company;
        let title = found.title.clone();
        self.remember();
        if let Some(evidence) = self
            .evidence
            .iter_mut()
            .find(|evidence| evidence.source == source && evidence.paragraph == paragraph)
        {
            if !evidence.themes.iter().any(|held| held == theme) {
                evidence.themes.push(theme.to_owned());
            }
            let id = evidence.id;
            self.push_activity(
                company,
                format!("Tagged a passage as {theme}"),
                text,
                Some(title),
            );
            return Some(id);
        }
        let id = self.fresh();
        self.evidence.push(Evidence {
            id,
            source,
            company,
            paragraph,
            text: text.clone(),
            themes: vec![theme.to_owned()],
        });
        self.push_activity(
            company,
            format!("Tagged a passage as {theme}"),
            text,
            Some(title),
        );
        Some(id)
    }

    pub(crate) fn propose(&self, company: Id) -> Result<Proposal, AskError> {
        let Some(record) = self.company(company) else {
            return Err(AskError::NoEvidence);
        };
        let quotes = self.quotes(company);
        if quotes.is_empty() {
            return Err(AskError::NoEvidence);
        }
        Ok(self.proposal_from(record, &quotes))
    }

    fn quotes(&self, company: Id) -> Vec<Quote> {
        self.evidence_at(company)
            .into_iter()
            .take(3)
            .map(|evidence| Quote {
                id: evidence.id,
                source: evidence.source,
                paragraph: evidence.paragraph,
                text: evidence.text.clone(),
                themes: evidence.themes.clone(),
            })
            .collect()
    }

    fn proposal_from(&self, record: &Company, quotes: &[Quote]) -> Proposal {
        let blob = quotes
            .iter()
            .map(|quote| quote.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let themes = quote_themes(quotes);
        let buyer = self.buyer(record.id, &blob);
        let summary = quote_summary(record.name(), buyer.as_deref(), quotes, |source| {
            self.source(source)
                .map(|source| source.title.clone())
                .unwrap_or_else(|| "source".to_owned())
        });
        let buying = buying_signal(&themes, &blob);
        let pilot = blob.to_ascii_lowercase().contains("pilot");
        let company = record.id;
        Proposal {
            insight_body: summary.clone(),
            summary,
            stage: proposed_stage(buying, record.stage),
            next_interaction: proposed_next(pilot, buying, &record.next_interaction),
            task: proposed_task(pilot || buying, buyer.as_deref(), |title| {
                self.task_taken(company, title)
            }),
            insight_title: insight_title(pilot, &themes),
            evidence: quotes.iter().map(|quote| quote.id).collect(),
        }
    }

    fn task_taken(&self, company: Id, title: &str) -> bool {
        self.tasks
            .iter()
            .any(|task| task.company == company && task.title == title)
    }

    pub(crate) fn accept(&mut self, company: Id, proposal: &Proposal) {
        if self.company(company).is_none() {
            return;
        }
        self.remember();
        let mut changes = Vec::new();
        if let Some(stage) = proposal.stage
            && let Some(record) = self.companies.iter_mut().find(|item| item.id == company)
        {
            changes.push(format!(
                "Stage {} → {}",
                record.stage.label(),
                stage.label()
            ));
            record.stage = stage;
        }
        if let Some(next) = &proposal.next_interaction
            && let Some(record) = self.companies.iter_mut().find(|item| item.id == company)
        {
            changes.push(format!("Next interaction → {next}"));
            record.next_interaction = next.clone();
        }
        if let Some(title) = &proposal.task {
            let evidence = proposal.evidence.first().copied();
            let id = self.fresh();
            self.tasks.push(Task {
                id,
                company,
                title: title.clone(),
                evidence,
            });
            changes.push(format!("Task added: {title}"));
        }
        self.insights.push((
            company,
            Insight {
                title: proposal.insight_title.clone(),
                body: proposal.insight_body.clone(),
                evidence: proposal.evidence.clone(),
            },
        ));
        let source = proposal.evidence.first().and_then(|id| {
            self.evidence
                .iter()
                .find(|evidence| evidence.id == *id)
                .and_then(|evidence| self.source(evidence.source))
                .map(|source| source.title.clone())
        });
        let detail = if changes.is_empty() {
            proposal.summary.clone()
        } else {
            format!("{}\n{}", changes.join("\n"), proposal.summary)
        };
        self.push_activity(
            company,
            "Accepted a proposal from the evidence".into(),
            detail,
            source,
        );
    }

    fn attach_acme_interview(&mut self) {
        let Some(company) = self
            .companies
            .iter()
            .find(|company| company.domain == "acme.example")
            .map(|company| company.id)
        else {
            return;
        };
        let id = self.fresh();
        self.sources.push(Source {
            id,
            company,
            title: "Acme pilot interview".into(),
            paragraphs: vec![
                "Ada Chen walked us through the warehouse.".into(),
                "We need a pilot in Q2, and Ada is the buyer.".into(),
                "Ops wants the integration to stay on the existing line.".into(),
                "Budget sits with finance until the pilot is scoped.".into(),
            ],
            kind: MaterialKind::Text,
            file: None,
        });
        self.push_activity(
            company,
            "Attached Acme pilot interview".into(),
            "Four passages extracted. Tag one before asking for a summary.".into(),
            Some("Acme pilot interview".into()),
        );
    }

    fn buyer(&self, company: Id, text: &str) -> Option<String> {
        self.people_at(company)
            .into_iter()
            .find_map(|person| mentions(text, person.name()).then(|| person.name().to_owned()))
    }

    fn fresh(&mut self) -> Id {
        let id = Id(self.next);
        self.next += 1;
        id
    }

    fn remember(&mut self) {
        self.undo.push(Snapshot {
            next: self.next,
            companies: self.companies.clone(),
            people: self.people.clone(),
            notes: self.notes.clone(),
            sources: self.sources.clone(),
            evidence: self.evidence.clone(),
            tasks: self.tasks.clone(),
            insights: self.insights.clone(),
            activity: self.activity.clone(),
        });
    }

    fn push_activity(
        &mut self,
        company: Id,
        summary: String,
        detail: String,
        source: Option<String>,
    ) {
        self.activity.push((
            company,
            Activity {
                summary,
                detail,
                source,
            },
        ));
    }

    fn to_json(&self) -> Value {
        json!({
            "version": 1,
            "next": self.next,
            "companies": self.companies.iter().map(|company| json!({
                "id": company.id.0,
                "name": company.name,
                "domain": company.domain,
                "stage": company.stage.label(),
                "owner": company.owner,
                "employees": company.employees,
                "last": company.last_interaction,
                "next": company.next_interaction,
            })).collect::<Vec<_>>(),
            "people": self.people.iter().map(|person| json!({
                "id": person.id.0,
                "name": person.name,
                "email": person.email,
                "title": person.title,
                "company": person.company.0,
            })).collect::<Vec<_>>(),
            "notes": self.notes.iter().map(|(company, note)| json!({
                "company": company.0,
                "body": note.body,
            })).collect::<Vec<_>>(),
            "sources": self.sources.iter().map(|source| json!({
                "id": source.id.0,
                "company": source.company.0,
                "title": source.title,
                "paragraphs": source.paragraphs,
                "kind": source.kind.as_str(),
                "file": source.file,
            })).collect::<Vec<_>>(),
            "evidence": self.evidence.iter().map(|item| json!({
                "id": item.id.0,
                "source": item.source.0,
                "company": item.company.0,
                "paragraph": item.paragraph,
                "text": item.text,
                "themes": item.themes,
            })).collect::<Vec<_>>(),
            "tasks": self.tasks.iter().map(|task| json!({
                "id": task.id.0,
                "company": task.company.0,
                "title": task.title,
                "evidence": task.evidence.map(|id| id.0),
            })).collect::<Vec<_>>(),
            "insights": self.insights.iter().map(|(company, insight)| json!({
                "company": company.0,
                "title": insight.title,
                "body": insight.body,
                "evidence": insight.evidence.iter().map(|id| id.0).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "activity": self.activity.iter().map(|(company, activity)| json!({
                "company": company.0,
                "summary": activity.summary,
                "detail": activity.detail,
                "source": activity.source,
            })).collect::<Vec<_>>(),
            "report": {
                "companies": self.report.companies_added,
                "people": self.report.people_added,
                "duplicates": self.report.duplicates,
            },
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        if value.get("version").and_then(Value::as_u64) != Some(1) {
            return None;
        }
        assemble(value)
    }
}

fn assemble(value: &Value) -> Option<Store> {
    let directory = directory_from(value)?;
    let journal = journal_from(value)?;
    Some(Store {
        next: directory.next,
        companies: directory.companies,
        people: directory.people,
        notes: journal.notes,
        sources: journal.sources,
        evidence: journal.evidence,
        tasks: journal.tasks,
        insights: journal.insights,
        activity: journal.activity,
        report: journal.report,
        undo: Vec::new(),
    })
}

struct Directory {
    next: u64,
    companies: Vec<Company>,
    people: Vec<Person>,
}

struct Journal {
    notes: Vec<(Id, Note)>,
    sources: Vec<Source>,
    evidence: Vec<Evidence>,
    tasks: Vec<Task>,
    insights: Vec<(Id, Insight)>,
    activity: Vec<(Id, Activity)>,
    report: ImportReport,
}

fn directory_from(value: &Value) -> Option<Directory> {
    Some(Directory {
        next: value.get("next").and_then(Value::as_u64)?,
        companies: entries(value, "companies", company_from)?,
        people: entries(value, "people", person_from)?,
    })
}

fn journal_from(value: &Value) -> Option<Journal> {
    let records = records_from(value)?;
    let history = history_from(value)?;
    Some(Journal {
        notes: records.notes,
        sources: records.sources,
        evidence: records.evidence,
        tasks: records.tasks,
        insights: history.insights,
        activity: history.activity,
        report: history.report,
    })
}

struct Records {
    notes: Vec<(Id, Note)>,
    sources: Vec<Source>,
    evidence: Vec<Evidence>,
    tasks: Vec<Task>,
}

struct History {
    insights: Vec<(Id, Insight)>,
    activity: Vec<(Id, Activity)>,
    report: ImportReport,
}

fn records_from(value: &Value) -> Option<Records> {
    Some(Records {
        notes: entries(value, "notes", note_from)?,
        sources: entries(value, "sources", source_from)?,
        evidence: entries(value, "evidence", evidence_from)?,
        tasks: entries(value, "tasks", task_from)?,
    })
}

fn history_from(value: &Value) -> Option<History> {
    Some(History {
        insights: entries(value, "insights", insight_from)?,
        activity: entries(value, "activity", activity_from)?,
        report: report_from(value.get("report")?)?,
    })
}

fn number_id(value: &Value) -> Option<Id> {
    value.as_u64().map(Id)
}

fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn company_from(value: &Value) -> Option<Company> {
    let id = number_id(value.get("id")?)?;
    let name = text_field(value, "name")?.to_owned();
    let domain = text_field(value, "domain")?.to_owned();
    let facts = company_facts(value)?;
    Some(Company {
        id,
        name,
        domain,
        stage: facts.0,
        owner: facts.1,
        employees: facts.2,
        last_interaction: facts.3,
        next_interaction: facts.4,
    })
}

fn company_facts(value: &Value) -> Option<(Stage, String, u32, String, String)> {
    Some((
        Stage::parse(text_field(value, "stage")?)?,
        text_field(value, "owner")?.to_owned(),
        value.get("employees").and_then(Value::as_u64)? as u32,
        text_field(value, "last")?.to_owned(),
        text_field(value, "next")?.to_owned(),
    ))
}

fn person_from(value: &Value) -> Option<Person> {
    Some(Person {
        id: number_id(value.get("id")?)?,
        name: text_field(value, "name")?.to_owned(),
        email: text_field(value, "email")?.to_owned(),
        title: text_field(value, "title")?.to_owned(),
        company: number_id(value.get("company")?)?,
    })
}

fn note_from(value: &Value) -> Option<(Id, Note)> {
    Some((
        number_id(value.get("company")?)?,
        Note {
            body: text_field(value, "body")?.to_owned(),
        },
    ))
}

fn source_from(value: &Value) -> Option<Source> {
    let id = number_id(value.get("id")?)?;
    let company = number_id(value.get("company")?)?;
    let body = source_body(value)?;
    Some(Source {
        id,
        company,
        title: body.0,
        paragraphs: body.1,
        kind: body.2,
        file: body.3,
    })
}

fn source_body(value: &Value) -> Option<(String, Vec<String>, MaterialKind, Option<String>)> {
    Some((
        text_field(value, "title")?.to_owned(),
        string_list(value, "paragraphs")?,
        MaterialKind::parse(text_field(value, "kind")?)?,
        optional_string(value, "file")?,
    ))
}

fn evidence_from(value: &Value) -> Option<Evidence> {
    let ids = evidence_ids(value)?;
    let body = evidence_body(value)?;
    Some(Evidence {
        id: ids.0,
        source: ids.1,
        company: ids.2,
        paragraph: body.0,
        text: body.1,
        themes: body.2,
    })
}

fn evidence_ids(value: &Value) -> Option<(Id, Id, Id)> {
    Some((
        number_id(value.get("id")?)?,
        number_id(value.get("source")?)?,
        number_id(value.get("company")?)?,
    ))
}

fn evidence_body(value: &Value) -> Option<(usize, String, Vec<String>)> {
    Some((
        value.get("paragraph").and_then(Value::as_u64)? as usize,
        text_field(value, "text")?.to_owned(),
        string_list(value, "themes")?,
    ))
}

fn task_from(value: &Value) -> Option<Task> {
    let id = number_id(value.get("id")?)?;
    let company = number_id(value.get("company")?)?;
    Some(Task {
        id,
        company,
        title: text_field(value, "title")?.to_owned(),
        evidence: optional_id(value, "evidence")?,
    })
}

fn insight_from(value: &Value) -> Option<(Id, Insight)> {
    let evidence = value
        .get("evidence")?
        .as_array()?
        .iter()
        .map(number_id)
        .collect::<Option<Vec<_>>>()?;
    Some((
        number_id(value.get("company")?)?,
        Insight {
            title: text_field(value, "title")?.to_owned(),
            body: text_field(value, "body")?.to_owned(),
            evidence,
        },
    ))
}

fn activity_from(value: &Value) -> Option<(Id, Activity)> {
    Some((
        number_id(value.get("company")?)?,
        Activity {
            summary: text_field(value, "summary")?.to_owned(),
            detail: text_field(value, "detail")?.to_owned(),
            source: optional_string(value, "source")?,
        },
    ))
}

fn report_from(value: &Value) -> Option<ImportReport> {
    let duplicates = value
        .get("duplicates")?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(ImportReport {
        companies_added: value.get("companies").and_then(Value::as_u64)? as usize,
        people_added: value.get("people").and_then(Value::as_u64)? as usize,
        duplicates,
    })
}

fn entries<T>(value: &Value, key: &str, parse: fn(&Value) -> Option<T>) -> Option<Vec<T>> {
    value.get(key)?.as_array()?.iter().map(parse).collect()
}

fn string_list(value: &Value, key: &str) -> Option<Vec<String>> {
    value
        .get(key)?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn optional_string(value: &Value, key: &str) -> Option<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(name) => Some(Some(name.as_str()?.to_owned())),
    }
}

fn optional_id(value: &Value, key: &str) -> Option<Option<Id>> {
    match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(id) => Some(Some(number_id(id)?)),
    }
}

fn rows_for<T>(rows: &[T], company: Id, company_of: fn(&T) -> Id) -> Vec<&T> {
    rows.iter()
        .filter(|row| company_of(row) == company)
        .collect()
}

fn owned<'a, T>(rows: &'a [(Id, T)], company: Id) -> Vec<&'a T> {
    rows.iter()
        .filter(|(id, _)| *id == company)
        .map(|(_, item)| item)
        .collect()
}

struct Quote {
    id: Id,
    source: Id,
    paragraph: usize,
    text: String,
    themes: Vec<String>,
}

fn quote_themes(quotes: &[Quote]) -> Vec<&str> {
    quotes
        .iter()
        .flat_map(|quote| quote.themes.iter().map(String::as_str))
        .collect()
}

fn quote_summary(
    company: &str,
    buyer: Option<&str>,
    quotes: &[Quote],
    title_of: impl Fn(Id) -> String,
) -> String {
    let mut summary = format!("{company} — from the attached evidence.");
    if let Some(name) = buyer {
        summary = format!("{name} is named in the source. {summary}");
    }
    for quote in quotes {
        let title = title_of(quote.source);
        let _ = write!(
            summary,
            " “{}” — {title}, paragraph {}.",
            quote.text,
            quote.paragraph + 1
        );
    }
    summary
}

fn buying_signal(themes: &[&str], blob: &str) -> bool {
    themes.iter().any(|theme| *theme == "Buying signal")
        || blob.to_ascii_lowercase().contains("buyer")
}

fn proposed_stage(buying: bool, stage: Stage) -> Option<Stage> {
    if buying && stage == Stage::Lead {
        Some(Stage::Qualified)
    } else {
        None
    }
}

fn proposed_next(pilot: bool, buying: bool, next: &str) -> Option<String> {
    if !(pilot || buying) || !needs_follow_up(next) {
        return None;
    }
    Some(if pilot {
        "Schedule the Q2 pilot".to_owned()
    } else {
        "Follow up on the source".to_owned()
    })
}

fn proposed_task(want: bool, buyer: Option<&str>, taken: impl Fn(&str) -> bool) -> Option<String> {
    let name = buyer?;
    let title = format!("Send the pilot plan to {name}");
    if !want || taken(&title) {
        return None;
    }
    Some(title)
}

fn insight_title(pilot: bool, themes: &[&str]) -> String {
    if pilot {
        return "Q2 pilot".to_owned();
    }
    themes
        .first()
        .copied()
        .unwrap_or("From the source")
        .to_owned()
}

fn needs_follow_up(next: &str) -> bool {
    let next = next.trim();
    next.is_empty() || next == "—" || next == "-"
}

fn mentions(text: &str, name: &str) -> bool {
    let first = name.split_whitespace().next().unwrap_or(name);
    text.split(|c: char| !c.is_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case(first))
}

fn records(csv: &str) -> Vec<Vec<String>> {
    csv.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .skip(1)
        .map(|line| line.split(',').map(|cell| cell.trim().to_owned()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_skips_a_duplicate_domain() {
        let store = Store::sample();
        let report = store.report();
        assert_eq!(report.companies_added(), 4);
        assert_eq!(report.people_added(), 6);
        assert_eq!(report.duplicates(), ["acme.example"]);
        assert_eq!(store.companies(View::All).len(), 4);
    }

    #[test]
    fn needs_follow_up_is_a_saved_view() {
        let store = Store::sample();
        let names: Vec<_> = store
            .companies(View::NeedsFollowUp)
            .iter()
            .map(|company| company.name())
            .collect();
        assert_eq!(names, ["Acme Robotics", "Keel Studio"]);
    }

    #[test]
    fn a_summary_quotes_the_tagged_passage_and_names_the_buyer() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        let source = store.sources_at(company)[0].id();
        store.tag(source, 1, "Buying signal");
        let proposal = store.propose(company).unwrap();
        assert!(
            proposal
                .summary
                .contains("We need a pilot in Q2, and Ada is the buyer.")
        );
        assert!(proposal.summary.contains("paragraph 2"));
        assert_eq!(proposal.stage, Some(Stage::Qualified));
        assert_eq!(
            proposal.next_interaction.as_deref(),
            Some("Schedule the Q2 pilot")
        );
        assert_eq!(
            proposal.task.as_deref(),
            Some("Send the pilot plan to Ada Chen")
        );
        assert_eq!(proposal.evidence.len(), 1);
    }

    #[test]
    fn asking_without_a_tag_is_refused() {
        let store = Store::sample();
        let company = store.companies(View::All)[0].id();
        assert_eq!(store.propose(company), Err(AskError::NoEvidence));
    }

    #[test]
    fn accepting_then_undoing_restores_the_record() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        let source = store.sources_at(company)[0].id();
        store.tag(source, 1, "Buying signal");
        let proposal = store.propose(company).unwrap();
        store.accept(company, &proposal);
        assert_eq!(store.company(company).unwrap().stage(), Stage::Qualified);
        assert_eq!(store.tasks_at(company).len(), 1);
        assert_eq!(
            store.tasks_at(company)[0].evidence,
            proposal.evidence.first().copied()
        );
        assert_eq!(store.insights_at(company).len(), 1);
        let receipt = store.activity_at(company)[0];
        assert_eq!(receipt.summary(), "Accepted a proposal from the evidence");
        assert_eq!(receipt.source(), Some("Acme pilot interview"));
        assert!(receipt.detail().contains("We need a pilot in Q2"));
        assert!(store.undo());
        assert_eq!(store.company(company).unwrap().stage(), Stage::Lead);
        assert!(store.tasks_at(company).is_empty());
        assert!(store.insights_at(company).is_empty());
    }

    #[test]
    fn a_second_theme_does_not_create_a_second_span() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        let source = store.sources_at(company)[0].id();
        store.tag(source, 1, "Buying signal");
        store.tag(source, 1, "Timeline");
        let evidence = store.evidence_at(company);
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            evidence[0].themes(),
            ["Buying signal".to_owned(), "Timeline".to_owned()]
        );
    }

    #[test]
    fn a_highlight_can_be_opened_from_its_id() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        let source = store.sources_at(company)[0].id();
        let id = store.tag(source, 1, "Buying signal").unwrap();
        let found = store.evidence_by_id(id).unwrap();
        assert_eq!(found.id(), id);
        assert_eq!(found.company(), company);
        assert_eq!(found.source(), source);
        assert_eq!(found.paragraph(), 1);
        assert!(found.text().contains("Ada is the buyer"));
    }

    #[test]
    fn a_saved_store_comes_back_with_its_note_evidence_and_file() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        store.add_note(company, "Call Ada".into());
        let source = store
            .attach(
                company,
                "acme.json".into(),
                MaterialKind::Json,
                vec!["$.buyer: Ada Chen".into()],
                Some("1-acme.json".into()),
            )
            .unwrap();
        store.tag(source, 0, "Buying signal");
        let dir = std::env::temp_dir().join(format!(
            "frame-crm-save-{}-{}",
            std::process::id(),
            company.raw()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        store.save(&dir);
        let loaded = Store::load(&dir).unwrap();
        assert_eq!(loaded.notes_at(company)[0].body(), "Call Ada");
        let saved = loaded.source(source).unwrap();
        assert_eq!(saved.kind(), MaterialKind::Json);
        assert_eq!(saved.file(), Some("1-acme.json"));
        assert_eq!(saved.paragraphs(), ["$.buyer: Ada Chen"]);
        assert_eq!(
            loaded.evidence_at(company)[0].themes(),
            ["Buying signal".to_owned()]
        );
        assert!(!loaded.can_undo());
        std::fs::write(dir.join("crm.json"), "{").unwrap();
        assert!(Store::load(&dir).is_none());
        assert!(dir.join("crm.json.unreadable").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_buyer_without_a_pilot_gets_a_follow_up_once() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        let source = store
            .attach(
                company,
                "note".into(),
                MaterialKind::Text,
                vec!["Ada is the buyer.".into()],
                None,
            )
            .unwrap();
        store.tag(source, 0, "Stakeholder");
        let proposal = store.propose(company).unwrap();
        assert_eq!(proposal.stage, Some(Stage::Qualified));
        assert_eq!(
            proposal.next_interaction.as_deref(),
            Some("Follow up on the source")
        );
        assert_eq!(
            proposal.task.as_deref(),
            Some("Send the pilot plan to Ada Chen")
        );
        assert_eq!(proposal.insight_title, "Stakeholder");
        store.accept(company, &proposal);
        let again = store.propose(company).unwrap();
        assert!(again.stage.is_none());
        assert!(again.next_interaction.is_none());
        assert!(again.task.is_none());
    }

    #[test]
    fn a_passage_with_no_signal_stays_a_quote() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        let source = store
            .attach(
                company,
                "note".into(),
                MaterialKind::Text,
                vec!["The warehouse is cold.".into()],
                None,
            )
            .unwrap();
        store.tag(source, 0, "Risk");
        let proposal = store.propose(company).unwrap();
        assert!(proposal.stage.is_none());
        assert!(proposal.next_interaction.is_none());
        assert!(proposal.task.is_none());
        assert_eq!(proposal.insight_title, "Risk");
        assert!(proposal.summary.contains("paragraph 1"));
        assert!(!proposal.summary.contains("is named"));
    }

    #[test]
    fn an_empty_note_and_a_missing_passage_do_not_change_the_record() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        store.add_note(company, "   ".into());
        let source = store.sources_at(company)[0].id();
        assert!(store.tag(source, 9, "Risk").is_none());
        assert!(store.tag(source, 1, "  ").is_none());
        assert!(store.notes_at(company).is_empty());
        assert!(store.evidence_at(company).is_empty());
        assert!(!store.can_undo());
        store.set_stage(company, Stage::Lead);
        assert!(!store.can_undo());
        store.set_stage(company, Stage::Pilot);
        assert_eq!(store.company(company).unwrap().stage(), Stage::Pilot);
    }

    #[test]
    fn a_missing_record_file_stays_missing() {
        let dir = std::env::temp_dir().join(format!("frame-crm-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Store::load(&dir).is_none());
        assert!(!dir.join("crm.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_import_row_is_skipped() {
        let store = Store::import(
            "name,domain,stage,owner,employees,last,next\nOnly,one.example,nope,A,1,—,—\nShort\n",
            "Ghost,ghost@example,Role,Nobody\n",
        );
        assert!(store.companies(View::All).is_empty());
        assert!(store.people().is_empty());
    }

    #[test]
    fn stage_and_kind_names_round_trip() {
        for stage in Stage::ALL {
            assert_eq!(Stage::parse(stage.label()), Some(stage));
        }
        for kind in [
            MaterialKind::Text,
            MaterialKind::Json,
            MaterialKind::Table,
            MaterialKind::Database,
        ] {
            assert_eq!(MaterialKind::parse(kind.as_str()), Some(kind));
            assert!(!kind.caption().is_empty());
        }
    }

    #[test]
    fn saving_twice_leaves_the_same_record() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        store.add_note(company, "Call Ada".into());
        let dir = std::env::temp_dir().join(format!("frame-crm-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        store.save(&dir);
        store.save(&dir);
        let loaded = Store::load(&dir).unwrap();
        assert_eq!(loaded.notes_at(company)[0].body(), "Call Ada");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_accepted_proposal_comes_back_with_its_task_and_insight() {
        let mut store = Store::sample();
        let company = store.companies(View::All)[0].id();
        let source = store.sources_at(company)[0].id();
        store.tag(source, 1, "Buying signal");
        let proposal = store.propose(company).unwrap();
        store.accept(company, &proposal);
        let dir = std::env::temp_dir().join(format!("frame-crm-accept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        store.save(&dir);
        let loaded = Store::load(&dir).unwrap();
        assert_eq!(loaded.company(company).unwrap().stage(), Stage::Qualified);
        assert_eq!(
            loaded.tasks_at(company)[0].title(),
            "Send the pilot plan to Ada Chen"
        );
        assert!(loaded.tasks_at(company)[0].evidence().is_some());
        assert!(loaded.insights_at(company)[0].body().contains("buyer"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_attached_file_is_copied_beside_the_records() {
        let dir = std::env::temp_dir().join(format!("frame-crm-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("acme.json");
        std::fs::write(&source, "{\"buyer\":\"Ada\"}").unwrap();
        let attachment = crate::crm::files::prepare(&dir, &source).unwrap();
        let name = attachment.file;
        assert!(name.ends_with("/acme.json"));
        assert_eq!(
            std::fs::read_to_string(dir.join("files").join(name)).unwrap(),
            "{\"buyer\":\"Ada\"}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
