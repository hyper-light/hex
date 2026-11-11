/* The built Program's allocation units (AC-T7.4.1): hex-vulkan-guest-ready's own source compiled
 * again with its main renamed, so each size is its own types' on the ABI it was built for. Its
 * --max-allocation-bytes holds its three storage() calls, devices × (physical-device + observation)
 * and queue families × queue-family, which scripts/guest/vulkan-ready-manifest.py sums from these.
 * size-max is the Program's own largest --max-output-bytes: the self-test's and the calibration's
 * runs leave the frame's bound to the node's decoder (observer/driver.rs), which holds it. */
#define main hex_vulkan_guest_ready
#include "hex-vulkan-guest-ready.c"
#undef main
#include <stdio.h>

int main(void)
{
    return printf("physical-device %zu\nobservation %zu\nqueue-family %zu\nsize-max %zu\n",
                  sizeof(VkPhysicalDevice), sizeof(struct observation),
                  sizeof(VkQueueFamilyProperties), (size_t)SIZE_MAX) < 0;
}
