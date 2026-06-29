/*
 * mp3rgain #201 — reference harness.
 *
 * Reads a PCM dump produced by the `dump_golden_pcm` Rust test and prints
 * GetTitleGain() from the original gain_analysis.c. This is the value frozen
 * as GOLDEN_GAIN_DB in src/replaygain.rs. Local, one-time use only — it is not
 * built by Cargo and not shipped in the crate (tests/ is excluded).
 *
 * Dump format (little-endian): [u32 sample_rate][u32 frames]
 *                              [f64 left * frames][f64 right * frames]
 */
#include <stdio.h>
#include <stdlib.h>
#include "gain_analysis.h"

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "/tmp/rg_golden_pcm.bin";

    FILE *f = fopen(path, "rb");
    if (!f) {
        perror("fopen");
        return 1;
    }

    unsigned int sample_rate = 0, frames = 0;
    if (fread(&sample_rate, 4, 1, f) != 1 || fread(&frames, 4, 1, f) != 1) {
        fprintf(stderr, "failed to read header\n");
        return 1;
    }

    double *left = malloc((size_t)frames * sizeof(double));
    double *right = malloc((size_t)frames * sizeof(double));
    if (!left || !right) {
        fprintf(stderr, "out of memory\n");
        return 1;
    }
    if (fread(left, sizeof(double), frames, f) != frames ||
        fread(right, sizeof(double), frames, f) != frames) {
        fprintf(stderr, "failed to read %u frames\n", frames);
        return 1;
    }
    fclose(f);

    if (InitGainAnalysis((long)sample_rate) != INIT_GAIN_ANALYSIS_OK) {
        fprintf(stderr, "InitGainAnalysis failed for %u Hz\n", sample_rate);
        return 1;
    }
    if (AnalyzeSamples(left, right, frames, 2) != GAIN_ANALYSIS_OK) {
        fprintf(stderr, "AnalyzeSamples failed\n");
        return 1;
    }

    /* GetTitleGain() == PINK_REF - loudness; this is what mp3rgain's
     * `PINK_REF - analyzer.get_loudness()` must reproduce. */
    printf("%.17g\n", (double)GetTitleGain());

    free(left);
    free(right);
    return 0;
}
