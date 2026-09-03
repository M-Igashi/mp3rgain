use colored::*;
use mp3rgain::replaygain::{self, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::GAIN_STEP_DB;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn print_version() {
    println!("mp3rgain version {}", VERSION);
    println!("A modern mp3gain replacement written in Rust");
    println!();
    println!("Each gain step = {:.4} dB", GAIN_STEP_DB);
}

pub fn print_usage() {
    println!("{} version {}", "mp3rgain".green().bold(), VERSION);
    println!("Lossless MP3 volume adjustment - a modern mp3gain replacement");
    println!();
    println!("{}", "USAGE:".cyan().bold());
    println!("    mp3rgain [OPTIONS] <FILES>...");
    println!();
    println!("{}", "OPTIONS:".cyan().bold());
    println!(
        "    -g <i>      Apply gain of i steps (each step = {:.4} dB)",
        GAIN_STEP_DB
    );
    println!("    -d <n>      Modify suggested/target gain by n dB (rounded to nearest step)");
    println!("    -l <c> <g>  Apply gain to left (0) or right (1) channel only");
    println!("    -m <i>      Modify suggested gain by integer i");
    println!("    -r          Apply Track gain (ReplayGain analysis)");
    println!("    -a          Apply Album gain (ReplayGain analysis)");
    println!("    --rg2       ReplayGain 2.0 analysis (BS.1770, -18 LUFS reference)");
    println!("    --r128      EBU R128 analysis (BS.1770, -23 LUFS target)");
    println!("    --true-peak Measure true peak (BS.1770-4 Annex 2) for REPLAYGAIN_*_PEAK");
    println!("                (requires --rg2 or --r128; default is sample peak)");
    println!("    --tags-only Write REPLAYGAIN_* tags without changing the audio, so the");
    println!("                listener can still turn ReplayGain off in their player.");
    println!("                Needs -r/-a; no undo tag is written (nothing to undo)");
    println!("    -e          Skip album analysis (even with multiple files)");
    println!("    -i <n>      Specify which audio track to process (default: 0)");
    println!("    -u          Undo gain changes (restore from APEv2 tag)");
    println!("    -x          Only find max amplitude of file");
    println!("    -s <mode>   Stored tag handling:");
    println!("                  c = check/show stored tag info");
    println!("                  d = delete stored tag info (with -u: undo gain first)");
    println!("                  s = skip (don't write) stored tag info");
    println!("                  r = force recalculation (default; kept for compatibility)");
    println!("                  R = reuse stored ReplayGain tags with -r/-a, re-analyzing");
    println!("                      only when tags are missing (mp3gain's default behavior)");
    println!("                  i = put all tags in ID3v2");
    println!("                  a = put all tags in APEv2 (mp3gain-identical)");
    println!("    -p          Preserve original file timestamp");
    println!("    -c          Ignore clipping warnings");
    println!("    -k          Prevent clipping (automatically limit gain)");
    println!("    -w          Wrap gain values (instead of clamping)");
    println!("    -t          Use temp file for writing (always on; kept for compatibility)");
    println!("    -f          Assume MPEG 2 Layer III (compatibility, no effect)");
    println!("    -q          Quiet mode (less output)");
    println!("    -R          Process directories recursively");
    println!("    -n          Dry-run mode (show what would be done)");
    println!("    --dry-run   Same as -n");
    println!("    --skip-errors  Skip files that fail to analyze instead of");
    println!("                   aborting (useful for `-a` on large libraries)");
    println!("    -j <n>      Worker threads for analysis (default: auto, 0=auto, 1=serial)");
    println!("    --threads <n>  Same as -j (also honors MP3RGAIN_THREADS env var)");
    println!("    -o <fmt>    Output format: 'text' (default), 'json', or 'tsv'");
    println!("    -v          Show version");
    println!("    -h          Show this help");
    println!();
    println!("{}", "EXAMPLES:".cyan().bold());
    println!("    mp3rgain song.mp3              Show file info");
    println!("    mp3rgain -g 2 song.mp3         Apply +2 steps (+3.01 dB)");
    println!("    mp3rgain -g -3 song.mp3        Apply -3 steps (-4.52 dB)");
    println!("    mp3rgain -r -d 4.5 song.mp3    Apply track gain, target +4.5 dB louder");
    println!("    mp3rgain -r song.mp3           Analyze and apply track gain");
    println!("    mp3rgain -a *.mp3              Analyze and apply album gain");
    println!("    mp3rgain -r --tags-only *.mp3  Tag with track gain, leave audio alone");
    println!("    mp3rgain -a --tags-only *.mp3  Tag with album+track gain (loudgain style)");
    println!("    mp3rgain -r --rg2 song.mp3     Track gain via ReplayGain 2.0 (BS.1770)");
    println!("    mp3rgain -a --r128 *.mp3       Album gain to the EBU R128 target");
    println!("    mp3rgain -r -m 2 *.mp3         Apply track gain + 2 steps");
    println!("    mp3rgain -e *.mp3              Track gain only (skip album calc)");
    println!("    mp3rgain -u song.mp3           Undo previous gain changes");
    println!("    mp3rgain -x song.mp3           Show max amplitude only");
    println!("    mp3rgain -s R -a *.mp3         Album gain from stored tags (rescan only");
    println!("                                   if any file lacks them)");
    println!("    mp3rgain -s c *.mp3            Check stored tag info");
    println!("    mp3rgain -s d *.mp3            Delete stored tag info");
    println!("    mp3rgain -g 2 -p song.mp3      Apply gain, preserve timestamp");
    println!("    mp3rgain -k -g 5 song.mp3      Apply gain with clipping prevention");
    println!("    mp3rgain -w -g 10 song.mp3     Apply gain with wrapping");
    println!("    mp3rgain -t -g 2 song.mp3      Apply gain using temp file");
    println!("    mp3rgain -R /path/to/music     Process directory recursively");
    println!("    mp3rgain -Rpa --skip-errors /music  Album-scan a library, skipping");
    println!("                                        files that fail to decode");
    println!("    mp3rgain -n -g 2 *.mp3         Dry-run (preview changes)");
    println!("    mp3rgain -o json song.mp3      Output in JSON format");
    println!("    mp3rgain -o tsv *.mp3          Output in tab-separated format");
    println!("    mp3rgain -o tsv -a */*.mp3     TSV rows while applying album gain");
    println!("    mp3rgain -l 0 3 song.mp3       Apply +3 steps to left channel");
    println!("    mp3rgain -l 1 -2 song.mp3      Apply -2 steps to right channel");
    println!();
    println!("{}", "NOTES:".cyan().bold());
    println!(
        "    - Each gain step = {:.4} dB (fixed by MP3 specification)",
        GAIN_STEP_DB
    );
    println!("    - Changes are lossless and reversible");
    println!("    - MP3 tags: REPLAYGAIN_* in ID3v2 (where players look),");
    println!("      MP3GAIN_UNDO / MINMAX in APEv2 (where mp3gain looks)");
    println!(
        "    - Progress bar shown automatically for {}+ files",
        crate::progress::PROGRESS_THRESHOLD
    );
    if replaygain::is_available() {
        println!(
            "    - ReplayGain analysis is {} (target: {} dB)",
            "enabled".green(),
            REPLAYGAIN_REFERENCE_DB
        );
        println!("    - Default analysis is ReplayGain 1.0 (mp3gain-identical values);");
        println!("      --rg2 / --r128 opt into BS.1770 loudness measurement");
    } else {
        println!();
        println!("{}", "REPLAYGAIN:".yellow().bold());
        println!("    -r and -a options require the 'replaygain' feature:");
        println!("    cargo install mp3rgain --features replaygain");
    }
}
