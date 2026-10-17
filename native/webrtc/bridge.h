#pragma once
#include <stdint.h>
#ifdef __cplusplus
#define FINCH_API extern "C" __declspec(dllexport)
#else
#define FINCH_API __declspec(dllimport)
#endif
// Handles and their 48 kHz mono buffers stay on one worker. No C++ ownership crosses ABI.
FINCH_API int finch_webrtc_create(int stage, void** output);
FINCH_API void finch_webrtc_destroy(void* handle);
FINCH_API int finch_webrtc_capture(void* handle, const float* input, float* output, uint32_t length);
FINCH_API int finch_webrtc_render(void* handle, const float* input, uint32_t length);
FINCH_API int finch_webrtc_reset(void* handle);
FINCH_API int finch_webrtc_set_delay(void* handle, uint32_t milliseconds);
FINCH_API int finch_webrtc_set_fixed_gain(void* handle, float decibels);
