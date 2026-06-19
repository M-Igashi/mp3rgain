#!/bin/bash
# compatibility-test.sh - Verify mp3rgain produces identical output to original mp3gain
#
# This script tests binary compatibility between mp3rgain and the original mp3gain.
# It applies the same operations to identical MP3 files using both tools and compares
# the output file hashes to verify they are identical.
#
# Usage:
#   ./scripts/compatibility-test.sh
#
# Environment variables:
#   MP3GAIN_BIN  - Path to original mp3gain binary (default: mp3gain)
#   MP3RGAIN_BIN - Path to mp3rgain binary (default: mp3rgain or cargo build)
#   TEST_DIR     - Directory for test files (default: tests/fixtures)
#   VERBOSE      - Set to 1 for verbose output

set -e

# Configuration
MP3GAIN_BIN="${MP3GAIN_BIN:-mp3gain}"
MP3RGAIN_BIN="${MP3RGAIN_BIN:-}"
TEST_DIR="${TEST_DIR:-tests/fixtures}"
TEMP_DIR=$(mktemp -d)
VERBOSE="${VERBOSE:-0}"
# Max allowed difference in the recommended ReplayGain track dB change.
DB_TOLERANCE="${DB_TOLERANCE:-0.1}"
RESULTS_FILE="${TEMP_DIR}/results.json"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0

# Cleanup on exit
cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

log() {
    echo -e "$1"
}

log_verbose() {
    if [ "$VERBOSE" = "1" ]; then
        echo -e "$1"
    fi
}

# Check if mp3gain is available
check_mp3gain() {
    if ! command -v "$MP3GAIN_BIN" &> /dev/null; then
        log "${YELLOW}Warning: Original mp3gain not found at '$MP3GAIN_BIN'${NC}"
        log "Install mp3gain to run compatibility tests:"
        log "  Ubuntu/Debian: sudo apt-get install mp3gain"
        log "  macOS: brew install mp3gain (deprecated, may not be available)"
        log ""
        log "You can also set MP3GAIN_BIN environment variable to specify the path."
        return 1
    fi

    local version
    version=$("$MP3GAIN_BIN" -v 2>&1 | head -1 || echo "unknown")
    log "Original mp3gain: $MP3GAIN_BIN"
    log "  Version: $version"
    return 0
}

# Check if mp3rgain is available
check_mp3rgain() {
    # If not specified, try to find mp3rgain
    if [ -z "$MP3RGAIN_BIN" ]; then
        # Check if running from project root
        if [ -f "Cargo.toml" ]; then
            # Build if needed
            log "Building mp3rgain..."
            cargo build --release --quiet
            MP3RGAIN_BIN="./target/release/mp3rgain"
        elif command -v mp3rgain &> /dev/null; then
            MP3RGAIN_BIN="mp3rgain"
        else
            log "${RED}Error: mp3rgain not found${NC}"
            return 1
        fi
    fi

    if [ ! -x "$MP3RGAIN_BIN" ] && ! command -v "$MP3RGAIN_BIN" &> /dev/null; then
        log "${RED}Error: mp3rgain not found at '$MP3RGAIN_BIN'${NC}"
        return 1
    fi

    local version
    version=$("$MP3RGAIN_BIN" -v 2>&1 | head -1 || echo "unknown")
    log "mp3rgain: $MP3RGAIN_BIN"
    log "  Version: $version"
    return 0
}

# Get hash of file (platform-independent)
get_hash() {
    local file="$1"
    if command -v sha256sum &> /dev/null; then
        sha256sum "$file" | cut -d' ' -f1
    elif command -v shasum &> /dev/null; then
        shasum -a 256 "$file" | cut -d' ' -f1
    else
        log "${RED}Error: No hash utility found (sha256sum or shasum)${NC}"
        exit 1
    fi
}

# Run a single test case
run_test() {
    local test_name="$1"
    local mp3_file="$2"
    shift 2
    local args=("$@")

    local basename
    basename=$(basename "$mp3_file")
    local test_original="${TEMP_DIR}/original_${basename}"
    local test_new="${TEMP_DIR}/new_${basename}"

    # Copy test files
    cp "$mp3_file" "$test_original"
    cp "$mp3_file" "$test_new"

    log_verbose "  Running: ${args[*]}"

    # Apply with original mp3gain
    # Use -s s to skip tag writing for pure audio comparison
    if ! "$MP3GAIN_BIN" -s s "${args[@]}" "$test_original" > /dev/null 2>&1; then
        log "  ${YELLOW}SKIP${NC}: mp3gain failed on this test"
        SKIP_COUNT=$((SKIP_COUNT + 1))
        return 0
    fi

    # Apply with mp3rgain
    # Use -s s to skip tag writing for pure audio comparison
    if ! "$MP3RGAIN_BIN" -s s "${args[@]}" "$test_new" > /dev/null 2>&1; then
        log "  ${RED}FAIL${NC}: mp3rgain failed on this test"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        return 1
    fi

    # Compare hashes
    local hash_original
    local hash_new
    hash_original=$(get_hash "$test_original")
    hash_new=$(get_hash "$test_new")

    if [ "$hash_original" = "$hash_new" ]; then
        log "  ${GREEN}PASS${NC}: $test_name"
        log_verbose "    Hash: $hash_original"
        PASS_COUNT=$((PASS_COUNT + 1))
        return 0
    else
        log "  ${RED}FAIL${NC}: $test_name - hashes differ"
        log "    mp3gain:  $hash_original"
        log "    mp3rgain: $hash_new"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        return 1
    fi
}

# Test gain application with -g flag
test_gain_steps() {
    local mp3_file="$1"
    local basename
    basename=$(basename "$mp3_file" .mp3)

    log ""
    log "Testing gain steps on: $basename"

    for gain in -5 -3 -1 1 2 3 5; do
        run_test "gain $gain steps" "$mp3_file" -g "$gain"
    done
}

# Note: -d option test removed because mp3gain's -d modifies "suggested gain"
# (used with ReplayGain), while mp3rgain's -d directly applies dB gain.
# This is a documented difference, not a compatibility issue.

# Note: Undo test removed because mp3gain and mp3rgain handle APE tags
# differently after undo (mp3gain keeps empty tags, mp3rgain removes them).
# The core undo functionality works correctly - only tag cleanup differs.

# Test clipping prevention
test_clipping_prevention() {
    local mp3_file="$1"
    local basename
    basename=$(basename "$mp3_file" .mp3)

    log ""
    log "Testing clipping prevention on: $basename"

    run_test "clipping prevention (-k -g 10)" "$mp3_file" -k -g 10
}

# Test channel-specific gain
test_channel_gain() {
    local mp3_file="$1"
    local basename
    basename=$(basename "$mp3_file" .mp3)

    # Skip mono files
    if [[ "$basename" == *"mono"* ]]; then
        log ""
        log "Skipping channel gain test on mono file: $basename"
        return 0
    fi

    log ""
    log "Testing channel-specific gain on: $basename"

    run_test "left channel +2" "$mp3_file" -l 0 2
    run_test "right channel -2" "$mp3_file" -l 1 -2
}

# Extract "<MP3 gain steps>\t<dB gain>" from the first data row of the
# tab-delimited (-o) analysis output. Both tools share the same column layout:
#   File  MP3 gain  dB gain  Max Amplitude  Max global_gain  Min global_gain
analysis_row() {
    awk -F'\t' 'NR==2 {print $2 "\t" $3; exit}'
}

# Compare the ReplayGain *analysis* — the recommended gain produced by the
# loudness algorithm itself, not the byte output of applying an explicit -g.
# This is the part the equal-loudness/RMS/percentile code actually computes.
test_replaygain_analysis() {
    local mp3_file="$1"
    local basename
    basename=$(basename "$mp3_file" .mp3)
    local copy_g="${TEMP_DIR}/an_g_${basename}.mp3"
    local copy_r="${TEMP_DIR}/an_r_${basename}.mp3"
    cp "$mp3_file" "$copy_g"
    cp "$mp3_file" "$copy_r"

    local g_row r_row
    if ! g_row=$("$MP3GAIN_BIN" -s s -o "$copy_g" 2>/dev/null | analysis_row); then
        log "  ${YELLOW}SKIP${NC}: mp3gain analysis failed on $basename"
        SKIP_COUNT=$((SKIP_COUNT + 1))
        return 0
    fi
    if ! r_row=$("$MP3RGAIN_BIN" -o tsv "$copy_r" 2>/dev/null | analysis_row); then
        log "  ${RED}FAIL${NC}: mp3rgain analysis failed on $basename"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        return 1
    fi

    local g_steps g_db r_steps r_db step_diff
    g_steps=$(printf '%s' "$g_row" | cut -f1)
    g_db=$(printf '%s' "$g_row" | cut -f2)
    r_steps=$(printf '%s' "$r_row" | cut -f1)
    r_db=$(printf '%s' "$r_row" | cut -f2)
    step_diff=$(( g_steps > r_steps ? g_steps - r_steps : r_steps - g_steps ))

    # dB within tolerance AND the quantized step within one 1.5 dB increment.
    if awk -v a="$g_db" -v b="$r_db" -v t="$DB_TOLERANCE" \
        'BEGIN { d = a - b; if (d < 0) d = -d; exit (d <= t) ? 0 : 1 }' \
        && [ "$step_diff" -le 1 ]; then
        local exact=""
        [ "$g_steps" = "$r_steps" ] && exact=" (exact step match)"
        log "  ${GREEN}PASS${NC}: analysis $basename — mp3gain ${g_db} dB / ${g_steps} steps vs mp3rgain ${r_db} dB / ${r_steps} steps${exact}"
        PASS_COUNT=$((PASS_COUNT + 1))
        return 0
    else
        log "  ${RED}FAIL${NC}: analysis $basename — recommended gain diverged"
        log "    mp3gain : ${g_db} dB / ${g_steps} steps"
        log "    mp3rgain: ${r_db} dB / ${r_steps} steps  [tolerance ${DB_TOLERANCE} dB, max 1 step]"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        return 1
    fi
}

# Main test execution
main() {
    log "=========================================="
    log "mp3rgain Compatibility Test Suite"
    log "=========================================="
    log ""

    # Check prerequisites
    if ! check_mp3gain; then
        log ""
        log "${YELLOW}Skipping compatibility tests (mp3gain not available)${NC}"
        log "Run this test on a system with mp3gain installed."
        exit 0
    fi

    log ""

    if ! check_mp3rgain; then
        exit 1
    fi

    log ""
    log "Test directory: $TEST_DIR"
    log "Temp directory: $TEMP_DIR"

    # Find test files
    if [ ! -d "$TEST_DIR" ]; then
        log "${RED}Error: Test directory not found: $TEST_DIR${NC}"
        exit 1
    fi

    local mp3_files=()
    while IFS= read -r -d '' file; do
        mp3_files+=("$file")
    done < <(find "$TEST_DIR" -name "*.mp3" -type f -print0 2>/dev/null)

    if [ ${#mp3_files[@]} -eq 0 ]; then
        log "${YELLOW}No MP3 files found in $TEST_DIR${NC}"
        log "Generate test fixtures first:"
        log "  ffmpeg -f lavfi -i \"sine=frequency=440:duration=1\" -ac 2 tests/fixtures/test_stereo.mp3"
        exit 1
    fi

    log "Found ${#mp3_files[@]} MP3 file(s) for testing"

    # Run tests on each file
    for mp3 in "${mp3_files[@]}"; do
        test_gain_steps "$mp3"
        test_clipping_prevention "$mp3"
        test_channel_gain "$mp3"
    done

    # Cross-check the ReplayGain analysis (recommended gain) against mp3gain.
    # The tests above verify gain *application* is byte-identical; this verifies
    # the loudness *calculation* agrees with the reference implementation.
    log ""
    log "Comparing ReplayGain analysis (recommended gain) against mp3gain..."
    for mp3 in "${mp3_files[@]}"; do
        test_replaygain_analysis "$mp3"
    done

    # Summary
    log ""
    log "=========================================="
    log "Test Summary"
    log "=========================================="
    log "${GREEN}PASSED${NC}: $PASS_COUNT"
    log "${RED}FAILED${NC}: $FAIL_COUNT"
    log "${YELLOW}SKIPPED${NC}: $SKIP_COUNT"
    log ""

    if [ "$FAIL_COUNT" -gt 0 ]; then
        log "${RED}Some tests failed!${NC}"
        exit 1
    else
        log "${GREEN}All compatibility tests passed!${NC}"
        exit 0
    fi
}

main "$@"
