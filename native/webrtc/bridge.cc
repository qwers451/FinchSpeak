#include "bridge.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <memory>
#include <new>
#include "api/audio/audio_processing.h"
namespace {
constexpr uint32_t kSamples = 480;
constexpr int kInvalid = -1000;
constexpr int kException = -1001;
struct State {
  int stage;
  uint32_t delay = 0;
  float fixed_gain = 0;
  decltype(webrtc::AudioProcessingBuilder().Create()) processor;
  explicit State(int kind) : stage(kind) { Recreate(); }
  void Recreate() {
    webrtc::AudioProcessing::Config config;
    config.pipeline.maximum_internal_processing_rate = 48000;
    config.pipeline.multi_channel_capture = false;
    config.pipeline.multi_channel_render = false;
    config.echo_canceller.enabled = stage == 0;
    config.high_pass_filter.enabled = stage == 0;
    config.noise_suppression.enabled = false;
    config.gain_controller1.enabled = false;
    config.gain_controller2.enabled = stage == 1;
    auto& gain = config.gain_controller2;
    gain.input_volume_controller.enabled = false;
    gain.adaptive_digital.enabled = stage == 1;
    gain.adaptive_digital.headroom_db = 6;
    gain.adaptive_digital.max_gain_db = 12;
    gain.adaptive_digital.initial_gain_db = 0;
    gain.adaptive_digital.max_gain_change_db_per_second = 6;
    gain.adaptive_digital.max_output_noise_level_dbfs = -50;
    gain.fixed_digital.gain_db = fixed_gain;
    auto next = webrtc::AudioProcessingBuilder().SetConfig(config).Create();
    if (!next) throw std::bad_alloc();
    if (stage == 0 && next->set_stream_delay_ms(static_cast<int>(delay)) != 0) throw std::bad_alloc();
    processor = std::move(next);
  }
};
bool Valid(const float* input, uint32_t length) {
  return input && length == kSamples && std::all_of(input, input + length, [](float x) {
    return std::isfinite(x) && x >= -1 && x <= 1;
  });
}
}
int finch_webrtc_create(int stage, void** output) {
  if (!output) return kInvalid;
  *output = nullptr;
  if (stage != 0 && stage != 1) return kInvalid;
  try { *output = new State(stage); return 0; } catch (...) { return kException; }
}
void finch_webrtc_destroy(void* handle) { delete static_cast<State*>(handle); }
int finch_webrtc_capture(void* handle, const float* input, float* output, uint32_t length) {
  if (!handle || !output || !Valid(input, length)) return kInvalid;
  try {
    auto& state = *static_cast<State*>(handle);
    std::array<float, kSamples> scratch{};
    const float* src[] = {input};
    float* dst[] = {scratch.data()};
    webrtc::StreamConfig stream(48000, 1);
    if (state.stage == 0) state.processor->set_stream_delay_ms(static_cast<int>(state.delay));
    const int status = state.processor->ProcessStream(src, stream, stream, dst);
    if (status != 0) return status;
    if (!std::all_of(scratch.begin(), scratch.end(), [](float x) { return std::isfinite(x); })) return kInvalid;
    std::copy(scratch.begin(), scratch.end(), output);
    return 0;
  } catch (...) { return kException; }
}
int finch_webrtc_render(void* handle, const float* input, uint32_t length) {
  if (!handle || !Valid(input, length)) return kInvalid;
  try {
    auto& state = *static_cast<State*>(handle);
    if (state.stage != 0) return kInvalid;
    std::array<float, kSamples> scratch{};
    const float* src[] = {input};
    float* dst[] = {scratch.data()};
    webrtc::StreamConfig stream(48000, 1);
    return state.processor->ProcessReverseStream(src, stream, stream, dst);
  } catch (...) { return kException; }
}
int finch_webrtc_reset(void* handle) {
  if (!handle) return kInvalid;
  try { static_cast<State*>(handle)->Recreate(); return 0; } catch (...) { return kException; }
}
int finch_webrtc_set_delay(void* handle, uint32_t milliseconds) {
  if (!handle || milliseconds > 500) return kInvalid;
  static_cast<State*>(handle)->delay = milliseconds;
  return 0;
}
int finch_webrtc_set_fixed_gain(void* handle, float decibels) {
  if (!handle || !std::isfinite(decibels) || decibels < 0 || decibels > 20) return kInvalid;
  try {
    auto& state = *static_cast<State*>(handle);
    if (state.stage != 1) return kInvalid;
    if (state.fixed_gain != decibels) {
      if (!state.processor->PostRuntimeSetting(webrtc::AudioProcessing::RuntimeSetting::CreateCaptureFixedPostGain(decibels))) return -1004;
      state.fixed_gain = decibels;
    }
    return 0;
  } catch (...) { return kException; }
}
