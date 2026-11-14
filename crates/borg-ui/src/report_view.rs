//! The dialog that saves a report: the file to write, its format, and what the format holds.

use crate::{
    Theme,
    field_dialog::{self, FieldDialog},
};
use borg_core::{ReportField, ReportForm, ReportFormat, ReportSource};
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect};

const FIELDS: [ReportField; 2] = [ReportField::Path, ReportField::Format];

fn label(field: ReportField) -> &'static str {
    match field {
        ReportField::Path => tr!(report_path_label),
        ReportField::Format => tr!(report_format_label),
    }
}

/// What a file in the chosen format holds.
fn format_note(form: &ReportForm) -> &'static str {
    match (form.format, &form.source) {
        (ReportFormat::Markdown, _) => tr!(report_markdown_note),
        (ReportFormat::Json, _) => tr!(report_json_note),
        (ReportFormat::Csv, ReportSource::PortScan(_)) => tr!(report_port_csv_note),
        (ReportFormat::Csv, ReportSource::Neighbors(_)) => tr!(report_host_csv_note),
    }
}

fn notes(form: &ReportForm) -> Vec<(String, bool)> {
    let problem = form.error.clone().map(|error| (error, true));
    let saving = form.saving.as_ref().map(|path| (tr!(report_saving, path = path.display()), false));
    [problem, saving, Some((format_note(form).to_owned(), false))].into_iter().flatten().collect()
}

fn value(form: &ReportForm, field: ReportField, room: usize) -> String {
    let selected = form.field == field;
    match field {
        ReportField::Path => field_dialog::input(&form.path, selected, room),
        ReportField::Format => field_dialog::choice(form.format.name(), selected),
    }
}

pub(crate) fn draw(form: &ReportForm, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let dialog = FieldDialog {
        title: tr!(report_form_title),
        keys: tr!(report_form_keys),
        fields: FIELDS.iter().map(|&field| (label(field), field == form.field)).collect(),
        notes: notes(form),
    };
    field_dialog::draw(&dialog, |index, room| value(form, FIELDS[index], room), theme, screen, buf)
}
