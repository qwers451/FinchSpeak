project('finch-webrtc', 'cpp', version: '1.0', default_options: ['cpp_std=c++20', 'default_library=static', 'b_vscrt=mt'])
apm = subproject('apm')
apm_dep = apm.get_variable('audio_processing_dep')
platform_args = ['-DWEBRTC_WIN', '-DNOMINMAX', '-D_USE_MATH_DEFINES']
executable('finch-webrtc-probe', 'probe/main.cc', dependencies: apm_dep, cpp_args: platform_args)
fs = import('fs')
if fs.is_file('bridge.cc')
  executable('finch-webrtc-bridge-test', ['probe/bridge_test.cc', 'probe/allocation_counter.cc', 'bridge.cc'], dependencies: apm_dep, cpp_args: platform_args)
  shared_library('finch_webrtc', 'bridge.cc', dependencies: apm_dep, cpp_args: platform_args, install: false)
endif
