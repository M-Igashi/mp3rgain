use super::widgets::{caption, keep_together, segmented, Check};
use crate::app::{AlbumGrouping, Mp3rgainApp};
use mp3rgain::replaygain::{AnalysisMode, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::TagLayout;

/// Settings bar below the toolbar: what to measure against and what an album
/// is on the first row, how files are written on the second.
///
/// Everything is disabled while a worker is running so a half-applied batch
/// can't see its settings change mid-flight.
pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    let frame =
        egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(12, 8));
    egui::TopBottomPanel::top("options_panel")
        .frame(frame)
        .show(ctx, |ui| {
            ui.add_enabled_ui(!app.is_processing(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    analysis_and_target(app, ui);
                    group_gap(ui);
                    album_grouping(app, ui);
                });
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    write_options(app, ui);
                });
            });
        });
}

/// Space between two groups of settings. Space rather than a rule, which
/// would be left dangling at the end of a wrapped row.
fn group_gap(ui: &mut egui::Ui) {
    ui.add_space(16.0);
}

/// Loudness measurement mode (issue #272) and the Target on its scale.
fn analysis_and_target(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    let before = app.analysis_mode;
    keep_together(ui, "analysis", |ui| {
        caption(ui, tr!("Analysis:"));
        segmented(ui, |ui| analysis_segments(app, ui));
    });
    // Switching modes invalidates cached analysis results: RG1 and BS.1770
    // numbers live on different scales.
    if app.analysis_mode != before {
        app.invalidate_analysis_results();
    }
    keep_together(ui, "target", |ui| target(app, ui));
}

fn analysis_segments(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    ui.selectable_value(&mut app.analysis_mode, AnalysisMode::Rg1, tr!("RG 1.0"))
        .on_hover_text(tr!("ReplayGain 1.0 (default): mp3gain-compatible values, \
                 89 dB reference. Keeps re-scans consistent with \
                 libraries normalized by mp3gain."));
    ui.selectable_value(&mut app.analysis_mode, AnalysisMode::Rg2, tr!("RG 2.0"))
        .on_hover_text(tr!(
            "ReplayGain 2.0: BS.1770 integrated loudness, -18 LUFS \
                 reference (same perceived level as 89 dB). \
                 CLI --rg2."
        ));
    ui.selectable_value(&mut app.analysis_mode, AnalysisMode::R128, tr!("R128"))
        .on_hover_text(tr!("EBU R128: BS.1770 integrated loudness, -23 LUFS \
                 broadcast target. CLI --r128."));
}

fn target(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    // Target volume. Stored on the 89 dB scale and shown on the selected
    // mode's scale, so one offset carries across modes: 95 dB in RG 1.0 is
    // -12 LUFS in RG 2.0 (issues #272, #364).
    caption(ui, tr!("Target:"));
    let shift = app.analysis_mode.reference_level() - REPLAYGAIN_REFERENCE_DB;
    let mut shown = app.target_volume + shift;
    let resp = ui
        .add(
            egui::DragValue::new(&mut shown)
                .speed(0.1)
                .range(75.0 + shift..=100.0 + shift)
                .suffix(format!(" {}", app.analysis_mode.unit())),
        )
        .on_hover_text(tr!(
            "Level to normalize to. One setting shared by all analysis \
             modes as an offset from the reference: 95 dB in RG 1.0 is \
             -12 LUFS in RG 2.0 and -17 LUFS in R128. Equivalent to the \
             CLI's -d."
        ));
    // The gain columns are derived from the Target, so a sort on them must
    // be redone (issue #161 item 1).
    if resp.changed() {
        app.target_volume = shown - shift;
        app.mark_display_dirty();
    }
}

/// What one album is, for Album Analysis / Apply Album Gain. The CLI's
/// --album-by, in the same three units (issue #338).
fn album_grouping(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    keep_together(ui, "album", |ui| {
        caption(ui, tr!("Album:"));
        segmented(ui, |ui| album_segments(app, ui));
    });
}

fn album_segments(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    ui.selectable_value(
        &mut app.album_grouping,
        AlbumGrouping::Folder,
        tr!("Per folder"),
    )
    .on_hover_text(tr!(
        "Each folder is its own album. A release whose discs live in \
             subfolders gets one album gain per disc, so use By tags for \
             those. CLI --album-by=dir."
    ));
    ui.selectable_value(&mut app.album_grouping, AlbumGrouping::Tag, tr!("By tags"))
        .on_hover_text(tr!(
            "One album per release, read from ALBUMARTIST / ALBUM (and \
                 MUSICBRAINZ_ALBUMID when present), so discs in subfolders \
                 share one album gain. Files with no ALBUM tag fall back to \
                 their own folder. CLI --album-by=tag."
        ));
    ui.selectable_value(
        &mut app.album_grouping,
        AlbumGrouping::Single,
        tr!("Single album"),
    )
    .on_hover_text(tr!(
        "Treat every loaded file as one album, ignoring folders. \
             Select rows first to group just those, which is how to \
             handle a release that has no usable tags. CLI -a."
    ));
}

fn write_options(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    caption(ui, tr!("Options:"));

    ui.add(Check::new(
        &mut app.apply_options.prevent_clipping,
        tr!("Prevent clipping"),
    ))
    .on_hover_text(tr!(
        "Cap the applied gain at the available headroom so the \
             output cannot clip. Equivalent to the CLI's -k flag."
    ));

    ui.add(Check::new(
        &mut app.apply_options.preserve_timestamp,
        tr!("Preserve mtime"),
    ))
    .on_hover_text(tr!(
        "Restore the file's last-modified timestamp after writing \
             so it doesn't jump to the top of playlists sorted by date. \
             CLI -p."
    ));

    ui.add(Check::new(&mut app.apply_options.wrap, tr!("Wrap mode")))
        .on_hover_text(tr!(
            "Allow gain values to wrap around 0–255 instead of saturating. \
                     Disables the clipping check. CLI -w. Rarely useful — leave off."
        ));

    ui.add(Check::new(&mut app.apply_options.dry_run, tr!("Dry run")))
        .on_hover_text(tr!(
            "Preview Apply Track / Album Gain without modifying any file. \
                         The Status column shows the steps that would be applied. \
                         CLI -n."
        ));

    // Issue #302: GUI counterpart of the CLI's -s R. Reuse is only trusted
    // in RG 1.0 mode, so the box greys out in the BS.1770 modes (mirroring
    // -s R being ignored with --rg2 / --r128).
    ui.add_enabled(
        app.analysis_mode == AnalysisMode::Rg1,
        Check::new(&mut app.use_stored_tags, tr!("Use stored tags")),
    )
    .on_hover_text(tr!(
        "Apply Track / Album Gain from stored REPLAYGAIN_* tags instead \
                     of re-analyzing; only files without usable tags are rescanned. \
                     Album Gain reuses tags only when every file in the album \
                     carries a consistent set; any gap rescans the whole album. \
                     CLI -s R. RG 1.0 mode only."
    ));

    group_gap(ui);
    keep_together(ui, "mp3_tags", |ui| tag_layout(app, ui));
}

fn tag_layout(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    caption(ui, tr!("MP3 tags:"));
    let layout = &mut app.apply_options.tag_layout;
    egui::ComboBox::from_id_salt("tag_layout")
        .selected_text(match layout {
            TagLayout::Split => tr!("ReplayGain in ID3v2"),
            TagLayout::Ape => tr!("All in APEv2"),
            TagLayout::Id3v2 => tr!("All in ID3v2"),
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(layout, TagLayout::Split, tr!("ReplayGain in ID3v2"))
                .on_hover_text(tr!("Default. REPLAYGAIN_* in ID3v2 TXXX where players \
                                 look, MP3GAIN_UNDO / MINMAX in APEv2 where mp3gain \
                                 looks."));
            ui.selectable_value(layout, TagLayout::Ape, tr!("All in APEv2"))
                .on_hover_text(tr!("Byte-for-byte mp3gain behaviour. ReplayGain-aware \
                                     players will not find the values. CLI -s a."));
            ui.selectable_value(layout, TagLayout::Id3v2, tr!("All in ID3v2"))
                .on_hover_text(tr!("Undo and ReplayGain both in ID3v2 TXXX frames. \
                                     CLI -s i."));
        });
}
