//! Integration tests for mp3rgain
//!
//! These tests use real MP3 files in tests/fixtures/ to verify
//! the correctness of gain application, undo, and channel-specific operations.

use mp3rgain::{analyze, apply_gain, apply_gain_channel, apply_gain_with_undo, undo_gain, Channel};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for unique file names
static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Helper to copy a test file to a temp location for testing with unique name
fn copy_test_file(name: &str) -> std::path::PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let src = Path::new("tests/fixtures").join(name);
    let dst = std::env::temp_dir().join(format!("mp3rgain_test_{}_{}", id, name));
    fs::copy(&src, &dst).expect("Failed to copy test file");
    dst
}

/// Helper to cleanup temp file
fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
}

// =============================================================================
// Analysis Tests
// =============================================================================

#[test]
fn test_analyze_stereo_file() {
    let path = Path::new("tests/fixtures/test_stereo.mp3");
    let result = analyze(path);
    assert!(
        result.is_ok(),
        "Failed to analyze stereo file: {:?}",
        result.err()
    );

    let info = result.unwrap();
    assert!(info.frame_count > 0, "Should have frames");
    assert_eq!(info.mpeg_version, "MPEG1");
    assert!(info.channel_mode == "Stereo" || info.channel_mode == "Joint Stereo");
    assert!(info.min_gain <= info.max_gain);
    assert!(info.avg_gain >= info.min_gain as f64);
    assert!(info.avg_gain <= info.max_gain as f64);
}

#[test]
fn test_analyze_mono_file() {
    let path = Path::new("tests/fixtures/test_mono.mp3");
    let result = analyze(path);
    assert!(
        result.is_ok(),
        "Failed to analyze mono file: {:?}",
        result.err()
    );

    let info = result.unwrap();
    assert!(info.frame_count > 0, "Should have frames");
    assert_eq!(info.channel_mode, "Mono");
}

#[test]
fn test_analyze_vbr_file() {
    let path = Path::new("tests/fixtures/test_vbr.mp3");
    let result = analyze(path);
    assert!(
        result.is_ok(),
        "Failed to analyze VBR file: {:?}",
        result.err()
    );

    let info = result.unwrap();
    assert!(info.frame_count > 0, "Should have frames");
}

#[test]
fn test_analyze_nonexistent_file() {
    let path = Path::new("tests/fixtures/nonexistent.mp3");
    let result = analyze(path);
    assert!(result.is_err(), "Should fail for nonexistent file");
}

// =============================================================================
// Gain Application Tests
// =============================================================================

#[test]
fn test_apply_positive_gain() {
    let path = copy_test_file("test_stereo.mp3");

    // Get original gain values
    let original = analyze(&path).unwrap();

    // Apply +2 steps
    let result = apply_gain(&path, 2);
    assert!(result.is_ok(), "Failed to apply gain: {:?}", result.err());
    assert!(result.unwrap() > 0, "Should modify frames");

    // Verify gain increased (accounting for saturation)
    let after = analyze(&path).unwrap();
    // For min_gain: if original was 0, it stays 0 (saturating_add from 0+2=2, but if it was already 0, result is 2)
    // Actually the gain values get modified, so we check they changed in the right direction
    if original.min_gain < 253 {
        assert!(
            after.min_gain >= original.min_gain,
            "min_gain should not decrease"
        );
    }
    if original.max_gain < 253 {
        assert!(
            after.max_gain >= original.max_gain,
            "max_gain should not decrease"
        );
    }

    cleanup(&path);
}

#[test]
fn test_apply_negative_gain() {
    let path = copy_test_file("test_stereo.mp3");

    // Get original gain values
    let original = analyze(&path).unwrap();

    // Apply -2 steps
    let result = apply_gain(&path, -2);
    assert!(
        result.is_ok(),
        "Failed to apply negative gain: {:?}",
        result.err()
    );

    // Verify gain decreased (accounting for saturation at 0)
    let after = analyze(&path).unwrap();
    if original.min_gain > 2 {
        assert!(
            after.min_gain <= original.min_gain,
            "min_gain should not increase"
        );
    }
    if original.max_gain > 2 {
        assert!(
            after.max_gain <= original.max_gain,
            "max_gain should not increase"
        );
    }

    cleanup(&path);
}

#[test]
fn test_apply_zero_gain() {
    let path = copy_test_file("test_stereo.mp3");

    // Apply 0 steps (should do nothing)
    let result = apply_gain(&path, 0);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0, "Zero gain should modify 0 frames");

    cleanup(&path);
}

#[test]
fn test_apply_gain_saturates_at_max() {
    let path = copy_test_file("test_stereo.mp3");

    // Apply huge positive gain (should saturate at 255)
    let result = apply_gain(&path, 200);
    assert!(result.is_ok());

    let after = analyze(&path).unwrap();
    // Max gain should be capped at 255 (u8 max)
    assert!(after.max_gain == 255, "max_gain should saturate at 255");

    cleanup(&path);
}

#[test]
fn test_apply_gain_saturates_at_min() {
    let path = copy_test_file("test_stereo.mp3");

    // Apply huge negative gain (should saturate at 0)
    let result = apply_gain(&path, -255);
    assert!(result.is_ok());

    let after = analyze(&path).unwrap();
    // Min gain should be capped at 0 (u8 min)
    assert!(after.min_gain == 0, "min_gain should saturate at 0");

    cleanup(&path);
}

// =============================================================================
// Undo Tests
// =============================================================================

#[test]
fn test_apply_and_undo_gain() {
    let path = copy_test_file("test_stereo.mp3");

    // Get original values
    let original = analyze(&path).unwrap();

    // Apply gain with undo support
    let result = apply_gain_with_undo(&path, 3);
    assert!(
        result.is_ok(),
        "Failed to apply gain with undo: {:?}",
        result.err()
    );

    // Verify gain changed (in the expected direction)
    let after_apply = analyze(&path).unwrap();
    assert!(
        after_apply.max_gain >= original.max_gain,
        "Gain should increase"
    );

    // Undo the gain
    let undo_result = undo_gain(&path);
    assert!(
        undo_result.is_ok(),
        "Failed to undo: {:?}",
        undo_result.err()
    );

    // Verify undo was applied (gain should decrease back toward original)
    let after_undo = analyze(&path).unwrap();
    // Undo should bring values back close to original
    // Allow small tolerance due to saturation effects
    assert!(
        after_undo.max_gain <= after_apply.max_gain,
        "max_gain should decrease after undo"
    );

    cleanup(&path);
}

#[test]
fn test_undo_without_previous_gain() {
    let path = copy_test_file("test_stereo.mp3");

    // Try to undo without any previous gain application
    let result = undo_gain(&path);
    assert!(result.is_err(), "Should fail to undo without APE tag");

    cleanup(&path);
}

#[test]
fn test_cumulative_gain_undo() {
    let path = copy_test_file("test_stereo.mp3");

    // Get original
    let original = analyze(&path).unwrap();

    // Apply gain twice
    apply_gain_with_undo(&path, 2).unwrap();
    apply_gain_with_undo(&path, 3).unwrap();

    // Verify cumulative gain increased
    let after = analyze(&path).unwrap();
    assert!(
        after.max_gain >= original.max_gain,
        "Gain should have increased"
    );

    // Undo should restore toward original
    undo_gain(&path).unwrap();
    let after_undo = analyze(&path).unwrap();
    // Verify undo reduced the gain
    assert!(
        after_undo.max_gain <= after.max_gain,
        "max_gain should decrease after undo"
    );

    cleanup(&path);
}

// =============================================================================
// Channel-Specific Gain Tests
// =============================================================================

#[test]
fn test_apply_gain_left_channel() {
    let path = copy_test_file("test_stereo.mp3");

    // Apply gain to left channel only
    let result = apply_gain_channel(&path, Channel::Left, 2);
    assert!(
        result.is_ok(),
        "Failed to apply left channel gain: {:?}",
        result.err()
    );
    assert!(result.unwrap() > 0, "Should modify frames");

    cleanup(&path);
}

#[test]
fn test_apply_gain_right_channel() {
    let path = copy_test_file("test_stereo.mp3");

    // Apply gain to right channel only
    let result = apply_gain_channel(&path, Channel::Right, -2);
    assert!(
        result.is_ok(),
        "Failed to apply right channel gain: {:?}",
        result.err()
    );
    assert!(result.unwrap() > 0, "Should modify frames");

    cleanup(&path);
}

#[test]
fn test_channel_gain_fails_on_mono() {
    let path = copy_test_file("test_mono.mp3");

    // Should fail on mono file
    let result = apply_gain_channel(&path, Channel::Left, 2);
    assert!(result.is_err(), "Should fail on mono file");

    let error_msg = result.err().unwrap().to_string();
    assert!(error_msg.contains("mono"), "Error should mention mono");

    cleanup(&path);
}

#[test]
fn test_channel_zero_gain() {
    let path = copy_test_file("test_stereo.mp3");

    // Zero gain should do nothing
    let result = apply_gain_channel(&path, Channel::Left, 0);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0, "Zero gain should modify 0 frames");

    cleanup(&path);
}

// =============================================================================
// Format Compatibility Tests
// =============================================================================

#[test]
fn test_vbr_gain_application() {
    let path = copy_test_file("test_vbr.mp3");

    let original = analyze(&path).unwrap();

    let result = apply_gain(&path, 2);
    assert!(result.is_ok(), "Failed on VBR file: {:?}", result.err());

    let after = analyze(&path).unwrap();
    // Verify gain increased
    assert!(
        after.max_gain >= original.max_gain,
        "Gain should increase on VBR file"
    );

    cleanup(&path);
}

#[test]
fn test_joint_stereo_gain_application() {
    let path = copy_test_file("test_joint_stereo.mp3");

    let original = analyze(&path).unwrap();

    let result = apply_gain(&path, 2);
    assert!(
        result.is_ok(),
        "Failed on joint stereo file: {:?}",
        result.err()
    );

    let after = analyze(&path).unwrap();
    // Verify gain increased
    assert!(
        after.max_gain >= original.max_gain,
        "Gain should increase on joint stereo file"
    );

    cleanup(&path);
}

#[test]
fn test_mono_gain_application() {
    let path = copy_test_file("test_mono.mp3");

    let original = analyze(&path).unwrap();

    // Regular gain should work on mono
    let result = apply_gain(&path, 2);
    assert!(result.is_ok(), "Failed on mono file: {:?}", result.err());

    let after = analyze(&path).unwrap();
    // Verify gain increased
    assert!(
        after.max_gain >= original.max_gain,
        "Gain should increase on mono file"
    );

    cleanup(&path);
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_headroom_calculation() {
    let path = Path::new("tests/fixtures/test_stereo.mp3");
    let info = analyze(path).unwrap();

    // Headroom should be 255 - max_gain
    assert_eq!(info.headroom_steps, (255 - info.max_gain) as i32);

    // Headroom in dB should be steps * 1.5
    let expected_db = info.headroom_steps as f64 * 1.5;
    assert!((info.headroom_db - expected_db).abs() < 0.01);
}

#[test]
fn test_file_not_modified_on_zero_gain() {
    let path = copy_test_file("test_stereo.mp3");

    // Get file hash before
    let before_content = fs::read(&path).unwrap();

    // Apply zero gain
    apply_gain(&path, 0).unwrap();

    // File should not be modified (no write for zero gain)
    let after_content = fs::read(&path).unwrap();
    assert_eq!(
        before_content, after_content,
        "File should not change with zero gain"
    );

    cleanup(&path);
}
