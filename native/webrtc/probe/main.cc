#include <array>
#include <cmath>
#include <iostream>
#include "api/audio/audio_processing.h"
int main() {
  for (int iteration = 0; iteration < 100; ++iteration) {
    webrtc::AudioProcessing::Config aec_config;
    aec_config.echo_canceller.enabled = true;
    auto aec = webrtc::AudioProcessingBuilder().Create();
    if (!aec) return 1;
    aec->ApplyConfig(aec_config);
    webrtc::AudioProcessing::Config gain_config;
    gain_config.gain_controller2.enabled = true;
    gain_config.gain_controller2.adaptive_digital.enabled = true;
    gain_config.gain_controller2.adaptive_digital.initial_gain_db = 0;
    gain_config.gain_controller2.adaptive_digital.max_gain_db = 12;
    auto gain = webrtc::AudioProcessingBuilder().Create();
    if (!gain) return 2;
    gain->ApplyConfig(gain_config);
    webrtc::StreamConfig stream(48000, 1);
    std::array<float, 480> input{}, output{};
    const float* src[] = { input.data() };
    float* dst[] = { output.data() };
    for (int block = 0; block < 100; ++block) {
      if (aec->ProcessReverseStream(src, stream, stream, dst) != 0) return 3;
      aec->set_stream_delay_ms(0);
      if (aec->ProcessStream(src, stream, stream, dst) != 0) return 4;
      if (gain->ProcessStream(src, stream, stream, dst) != 0) return 5;
      for (float value : output) if (!std::isfinite(value)) return 6;
    }
  }
  std::cout << "PASS: original C++ WebRTC AEC3/AGC2, 100 create/drop cycles, 10000 mono blocks" << std::endl;
}
