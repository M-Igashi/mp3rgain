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
    if ! "$MP3GAIN_BIN" "${args[@]}" "$test_original" > /dev/null 2>&1; then
        log "  ${YELLOW}SKIP${NC}: mp3gain failed on this test"
        ((SKIP_COUNT++))
        return 0
    fi

    # Apply with mp3rgain
    if ! "$MP3RGAIN_BIN" "${args[@]}" "$test_new" > /dev/null 2>&1; then
        log "  ${RED}FAIL${NC}: mp3rgain failed on this test"
        ((FAIL_COUNT++))
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
        ((PASS_COUNT++))
        return 0
    else
        log "  ${RED}FAIL${NC}: $test_name - hashes differ"
        log "    mp3gain:  $hash_original"
        log "    mp3rgain: $hash_new"
        ((FAIL_COUNT++))
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

# Test dB gain with -d flag
test_gain_db() {
    local mp3_file="$1"
    local basename
    basename=$(basename "$mp3_file" .mp3)

    log ""
    log "Testing dB gain on: $basename"

    for db in -6.0 -4.5 -3.0 -1.5 1.5 3.0 4.5 6.0; do
        run_test "gain ${db}dB" "$mp3_file" -d "$db"
    done
}

# Test undo functionality
test_undo() {
    local mp3_file="$1"
    local basename
    basename=$(basename "$mp3_file" .mp3)

    log ""
    log "Testing undo on: $basename"

    local test_original="${TEMP_DIR}/undo_original_${basename}.mp3"
    local test_new="${TEMP_DIR}/undo_new_${basename}.mp3"

    # Copy test files
    cp "$mp3_file" "$test_original"
    cp "$mp3_file" "$test_new"

    # Apply gain then undo with mp3gain
    "$MP3GAIN_BIN" -g 3 "$test_original" > /dev/null 2>&1
    "$MP3GAIN_BIN" -u "$test_original" > /dev/null 2>&1

    # Apply gain then undo with mp3rgain
    "$MP3RGAIN_BIN" -g 3 "$test_new" > /dev/null 2>&1
    "$MP3RGAIN_BIN" -u "$test_new" > /dev/null 2>&1

    # Compare with original file
    local hash_source
    local hash_original
    local hash_new
    hash_source=$(get_hash "$mp3_file")
    hash_original=$(get_hash "$test_original")
    hash_new=$(get_hash "$test_new")

    # Check mp3gain undo
    if [ "$hash_source" != "$hash_original" ]; then
        log "  ${YELLOW}NOTE${NC}: mp3gain undo does not restore original (expected)"
    fi

    # Compare mp3gain and mp3rgain results
    if [ "$hash_original" = "$hash_new" ]; then
        log "  ${GREEN}PASS${NC}: undo produces identical results"
        ((PASS_COUNT++))
    else
        log "  ${RED}FAIL${NC}: undo results differ"
        log "    mp3gain:  $hash_original"
        log "    mp3rgain: $hash_new"
        ((FAIL_COUNT++))
    fi
}

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
        test_gain_db "$mp3"
        test_undo "$mp3"
        test_clipping_prevention "$mp3"
        test_channel_gain "$mp3"
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
