#include "../bridge.h"
#include <array>
#include <cmath>
#include <iostream>
#include <limits>
#define CHECK(condition) do { if (!(condition)) { std::cerr << "FAIL line " << __LINE__ << ": " << #condition << std::endl; return __LINE__; } } while (false)
extern "C" void finch_test_measure(bool enabled);
extern "C" unsigned long long finch_test_allocations();
int main() {
  void* invalid = nullptr;
  CHECK(finch_webrtc_create(99, &invalid) != 0 && invalid == nullptr);
  CHECK(finch_webrtc_create(0, nullptr) != 0);
  finch_webrtc_destroy(nullptr);
  for (int stage : {0, 1}) {
    for (int cycle = 0; cycle < 100; ++cycle) {
      void* processor = nullptr;
      CHECK(finch_webrtc_create(stage, &processor) == 0 && processor);
      std::array<float, 480> input{}, output{};
      CHECK(finch_webrtc_capture(processor, input.data(), output.data(), 479) != 0);
      CHECK(finch_webrtc_capture(nullptr, input.data(), output.data(), 480) != 0);
      CHECK(finch_webrtc_capture(processor, nullptr, output.data(), 480) != 0);
      input[0] = std::numeric_limits<float>::quiet_NaN();
      output.fill(0.25f);
      CHECK(finch_webrtc_capture(processor, input.data(), output.data(), 480) != 0);
      for (float value : output) CHECK(value == 0.25f);
      input.fill(0);
      CHECK(finch_webrtc_set_delay(processor, 40) == 0);
      if (stage == 1) {
        CHECK(finch_webrtc_set_fixed_gain(processor, 18) == 0);
        CHECK(finch_webrtc_set_fixed_gain(processor, -1) != 0);
        CHECK(finch_webrtc_set_fixed_gain(processor, std::numeric_limits<float>::infinity()) != 0);
      }
      for (int block = 0; block < 100; ++block) {
        if (stage == 0) CHECK(finch_webrtc_render(processor, input.data(), 480) == 0);
        CHECK(finch_webrtc_capture(processor, input.data(), output.data(), 480) == 0);
        for (float value : output) CHECK(std::isfinite(value) && value == 0);
      }
      finch_test_measure(true);
      for (int block = 0; block < 100; ++block) {
        if (stage == 0) CHECK(finch_webrtc_render(processor, input.data(), 480) == 0);
        CHECK(finch_webrtc_capture(processor, input.data(), output.data(), 480) == 0);
      }
      CHECK(finch_test_allocations() == 0);
      CHECK(finch_webrtc_reset(processor) == 0);
      if (stage == 1 && cycle == 0) {
        int refused = 0;
        for (int update = 0; update < 1000; ++update) {
          refused += finch_webrtc_set_fixed_gain(processor, update % 2 ? 18.0f : 0.0f) != 0;
        }
        CHECK(refused > 0); // Native queue failure must be observable, never a false success.
        CHECK(finch_webrtc_reset(processor) == 0);
      }
      finch_webrtc_destroy(processor);
    }
  }
  std::cout << "PASS: C ABI validation, invalid input, 200 create/drop/reset and 40000 blocks; no C++ new allocations after warm-up" << std::endl;
}
