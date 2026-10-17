// Test executable only: account C++ operator-new traffic after DSP warm-up.
#include <atomic>
#include <cstdlib>
#include <new>
static std::atomic<bool> measuring{false};
static std::atomic<unsigned long long> allocations{0};
extern "C" void finch_test_measure(bool enabled) { allocations.store(0); measuring.store(enabled); }
extern "C" unsigned long long finch_test_allocations() { measuring.store(false); return allocations.load(); }
void* operator new(std::size_t count) {
  if (measuring.load(std::memory_order_relaxed)) allocations.fetch_add(1, std::memory_order_relaxed);
  if (void* result = std::malloc(count ? count : 1)) return result;
  throw std::bad_alloc();
}
void* operator new[](std::size_t count) { return ::operator new(count); }
void operator delete(void* pointer) noexcept { std::free(pointer); }
void operator delete[](void* pointer) noexcept { std::free(pointer); }
void operator delete(void* pointer, std::size_t) noexcept { std::free(pointer); }
void operator delete[](void* pointer, std::size_t) noexcept { std::free(pointer); }
