/* A fake Venus Vulkan ICD for scripts/guest/vulkan-ready.sh's self-test (AC-T7.4.1): one virtual
 * GPU whose driver is MESA_VENUS ("venus"), one compute queue family, one device-local heap, and a
 * queue whose submit signals its fence at once. The real Vulkan loader loads it through an ICD
 * manifest, so the built Program runs through its closure's own interpreter, libc and loader as a
 * guest runs it; only the driver is a stand-in. Never part of a closure. Interface: the loader's
 * LoaderDriverInterface.md (vk_icdNegotiateLoaderICDInterfaceVersion, version 5). */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <vulkan/vulkan.h>

#define EXPORT __attribute__((visibility("default")))

/* A dispatchable handle's first word is the loader's (vk_icd.h's VK_LOADER_DATA): the ICD sets it
 * to ICD_LOADER_MAGIC and the loader overwrites it with its dispatch table. */
#define ICD_LOADER_MAGIC 0x01CDC0DE
struct dispatchable {
    uintptr_t loader;
};

struct instance {
    struct dispatchable base;
    struct dispatchable physical;
};

struct device {
    struct dispatchable base;
    struct dispatchable queue;
};

struct fence {
    int signalled;
};

static VKAPI_ATTR VkResult VKAPI_CALL instance_version(uint32_t *version)
{
    *version = VK_API_VERSION_1_3;
    return VK_SUCCESS;
}

static VKAPI_ATTR VkResult VKAPI_CALL instance_extensions(const char *layer, uint32_t *count,
                                                          VkExtensionProperties *properties)
{
    (void)layer;
    (void)properties;
    *count = 0;
    return VK_SUCCESS;
}

static VKAPI_ATTR VkResult VKAPI_CALL create_instance(const VkInstanceCreateInfo *info,
                                                      const VkAllocationCallbacks *allocator,
                                                      VkInstance *out)
{
    (void)info;
    (void)allocator;
    struct instance *instance = calloc(1, sizeof(*instance));
    if (instance == NULL)
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    instance->base.loader = ICD_LOADER_MAGIC;
    instance->physical.loader = ICD_LOADER_MAGIC;
    *out = (VkInstance)instance;
    return VK_SUCCESS;
}

static VKAPI_ATTR void VKAPI_CALL destroy_instance(VkInstance instance,
                                                   const VkAllocationCallbacks *allocator)
{
    (void)allocator;
    free((struct instance *)instance);
}

static VKAPI_ATTR VkResult VKAPI_CALL enumerate_physical(VkInstance handle, uint32_t *count,
                                                         VkPhysicalDevice *devices)
{
    struct instance *instance = (struct instance *)handle;
    if (devices == NULL) {
        *count = 1;
        return VK_SUCCESS;
    }
    if (*count < 1)
        return VK_INCOMPLETE;
    *count = 1;
    devices[0] = (VkPhysicalDevice)&instance->physical;
    return VK_SUCCESS;
}

static void fill_properties(VkPhysicalDeviceProperties *properties)
{
    const char name[] = "Virtio-GPU Venus (fake self-test device)";
    memset(properties, 0, sizeof(*properties));
    properties->apiVersion = VK_API_VERSION_1_3;
    properties->driverVersion = VK_MAKE_API_VERSION(0, 25, 0, 7);
    properties->vendorID = 0x1af4; /* virtio's PCI vendor */
    properties->deviceID = 0x1050; /* virtio-gpu's PCI device */
    properties->deviceType = VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU;
    memcpy(properties->deviceName, name, sizeof(name));
}

static VKAPI_ATTR void VKAPI_CALL physical_properties(VkPhysicalDevice device,
                                                      VkPhysicalDeviceProperties *properties)
{
    (void)device;
    fill_properties(properties);
}

static VKAPI_ATTR void VKAPI_CALL physical_properties2(VkPhysicalDevice device,
                                                       VkPhysicalDeviceProperties2 *properties)
{
    (void)device;
    fill_properties(&properties->properties);
    for (VkBaseOutStructure *next = properties->pNext; next != NULL; next = next->pNext) {
        if (next->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES) {
            VkPhysicalDeviceIDProperties *ids = (VkPhysicalDeviceIDProperties *)next;
            memset(ids->deviceUUID, 0xab, VK_UUID_SIZE);
            memset(ids->driverUUID, 0xcd, VK_UUID_SIZE);
            memset(ids->deviceLUID, 0, VK_LUID_SIZE);
            ids->deviceNodeMask = 0;
            ids->deviceLUIDValid = VK_FALSE;
        } else if (next->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES) {
            VkPhysicalDeviceDriverProperties *driver = (VkPhysicalDeviceDriverProperties *)next;
            const char venus[] = "venus";
            const char info[] = "Mesa 25.0.7 (fake self-test ICD)";
            driver->driverID = VK_DRIVER_ID_MESA_VENUS;
            memset(driver->driverName, 0, sizeof(driver->driverName));
            memcpy(driver->driverName, venus, sizeof(venus));
            memset(driver->driverInfo, 0, sizeof(driver->driverInfo));
            memcpy(driver->driverInfo, info, sizeof(info));
            driver->conformanceVersion = (VkConformanceVersion){1, 3, 0, 0};
        }
    }
}

static VKAPI_ATTR void VKAPI_CALL physical_memory(VkPhysicalDevice device,
                                                  VkPhysicalDeviceMemoryProperties *memory)
{
    (void)device;
    memset(memory, 0, sizeof(*memory));
    memory->memoryTypeCount = 1;
    memory->memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
    memory->memoryTypes[0].heapIndex = 0;
    memory->memoryHeapCount = 1;
    memory->memoryHeaps[0].size = UINT64_C(1) << 30;
    memory->memoryHeaps[0].flags = VK_MEMORY_HEAP_DEVICE_LOCAL_BIT;
}

static VKAPI_ATTR void VKAPI_CALL physical_queues(VkPhysicalDevice device, uint32_t *count,
                                                  VkQueueFamilyProperties *families)
{
    (void)device;
    if (families == NULL) {
        *count = 1;
        return;
    }
    if (*count < 1)
        return;
    *count = 1;
    memset(families, 0, sizeof(*families));
    families[0].queueFlags = VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT | VK_QUEUE_TRANSFER_BIT;
    families[0].queueCount = 1;
    families[0].timestampValidBits = 64;
    families[0].minImageTransferGranularity = (VkExtent3D){1, 1, 1};
}

/* The rest of the 1.0 physical-device commands every ICD exports (the loader skips one missing any):
 * no features, formats or sparse images, which the Program never asks for. */
static VKAPI_ATTR void VKAPI_CALL physical_features(VkPhysicalDevice device,
                                                    VkPhysicalDeviceFeatures *features)
{
    (void)device;
    memset(features, 0, sizeof(*features));
}

static VKAPI_ATTR void VKAPI_CALL physical_format(VkPhysicalDevice device, VkFormat format,
                                                  VkFormatProperties *properties)
{
    (void)device;
    (void)format;
    memset(properties, 0, sizeof(*properties));
}

static VKAPI_ATTR VkResult VKAPI_CALL physical_image_format(VkPhysicalDevice device, VkFormat format,
                                                            VkImageType type, VkImageTiling tiling,
                                                            VkImageUsageFlags usage,
                                                            VkImageCreateFlags flags,
                                                            VkImageFormatProperties *properties)
{
    (void)device;
    (void)format;
    (void)type;
    (void)tiling;
    (void)usage;
    (void)flags;
    memset(properties, 0, sizeof(*properties));
    return VK_ERROR_FORMAT_NOT_SUPPORTED;
}

static VKAPI_ATTR void VKAPI_CALL physical_sparse(VkPhysicalDevice device, VkFormat format,
                                                  VkImageType type, VkSampleCountFlagBits samples,
                                                  VkImageUsageFlags usage, VkImageTiling tiling,
                                                  uint32_t *count,
                                                  VkSparseImageFormatProperties *properties)
{
    (void)device;
    (void)format;
    (void)type;
    (void)samples;
    (void)usage;
    (void)tiling;
    (void)properties;
    *count = 0;
}

static VKAPI_ATTR VkResult VKAPI_CALL device_extensions(VkPhysicalDevice device, const char *layer,
                                                        uint32_t *count,
                                                        VkExtensionProperties *properties)
{
    (void)device;
    (void)layer;
    (void)properties;
    *count = 0;
    return VK_SUCCESS;
}

static VKAPI_ATTR VkResult VKAPI_CALL create_device(VkPhysicalDevice physical,
                                                    const VkDeviceCreateInfo *info,
                                                    const VkAllocationCallbacks *allocator,
                                                    VkDevice *out)
{
    (void)physical;
    (void)allocator;
    if (info->queueCreateInfoCount != 1 || info->pQueueCreateInfos[0].queueFamilyIndex != 0 ||
        info->pQueueCreateInfos[0].queueCount != 1)
        return VK_ERROR_INITIALIZATION_FAILED;
    struct device *device = calloc(1, sizeof(*device));
    if (device == NULL)
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    device->base.loader = ICD_LOADER_MAGIC;
    device->queue.loader = ICD_LOADER_MAGIC;
    *out = (VkDevice)device;
    return VK_SUCCESS;
}

static VKAPI_ATTR void VKAPI_CALL destroy_device(VkDevice device,
                                                 const VkAllocationCallbacks *allocator)
{
    (void)allocator;
    free((struct device *)device);
}

static VKAPI_ATTR void VKAPI_CALL device_queue(VkDevice handle, uint32_t family, uint32_t index,
                                               VkQueue *queue)
{
    struct device *device = (struct device *)handle;
    *queue = (family == 0 && index == 0) ? (VkQueue)&device->queue : VK_NULL_HANDLE;
}

static VKAPI_ATTR VkResult VKAPI_CALL create_fence(VkDevice device, const VkFenceCreateInfo *info,
                                                   const VkAllocationCallbacks *allocator,
                                                   VkFence *out)
{
    (void)device;
    (void)allocator;
    struct fence *fence = calloc(1, sizeof(*fence));
    if (fence == NULL)
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    fence->signalled = (info->flags & VK_FENCE_CREATE_SIGNALED_BIT) != 0;
    *out = (VkFence)fence;
    return VK_SUCCESS;
}

static VKAPI_ATTR void VKAPI_CALL destroy_fence(VkDevice device, VkFence fence,
                                                const VkAllocationCallbacks *allocator)
{
    (void)device;
    (void)allocator;
    free((struct fence *)fence);
}

static VKAPI_ATTR VkResult VKAPI_CALL queue_submit(VkQueue queue, uint32_t count,
                                                   const VkSubmitInfo *submits, VkFence fence)
{
    (void)queue;
    (void)count;
    (void)submits;
    if (fence != VK_NULL_HANDLE)
        ((struct fence *)fence)->signalled = 1;
    return VK_SUCCESS;
}

static VKAPI_ATTR VkResult VKAPI_CALL wait_fences(VkDevice device, uint32_t count,
                                                  const VkFence *fences, VkBool32 all,
                                                  uint64_t timeout)
{
    (void)device;
    (void)all;
    (void)timeout;
    for (uint32_t i = 0; i < count; ++i)
        if (!((struct fence *)fences[i])->signalled)
            return VK_TIMEOUT;
    return VK_SUCCESS;
}

static PFN_vkVoidFunction device_function(const char *name);

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL device_proc(VkDevice device, const char *name)
{
    (void)device;
    return device_function(name);
}

struct entry {
    const char *name;
    PFN_vkVoidFunction function;
};

#define ENTRY(name, function) {name, (PFN_vkVoidFunction)function}

static const struct entry device_entries[] = {
    ENTRY("vkGetDeviceProcAddr", device_proc),
    ENTRY("vkDestroyDevice", destroy_device),
    ENTRY("vkGetDeviceQueue", device_queue),
    ENTRY("vkCreateFence", create_fence),
    ENTRY("vkDestroyFence", destroy_fence),
    ENTRY("vkQueueSubmit", queue_submit),
    ENTRY("vkWaitForFences", wait_fences),
};

static const struct entry instance_entries[] = {
    ENTRY("vkEnumerateInstanceVersion", instance_version),
    ENTRY("vkEnumerateInstanceExtensionProperties", instance_extensions),
    ENTRY("vkCreateInstance", create_instance),
    ENTRY("vkDestroyInstance", destroy_instance),
    ENTRY("vkEnumeratePhysicalDevices", enumerate_physical),
    ENTRY("vkGetPhysicalDeviceProperties", physical_properties),
    ENTRY("vkGetPhysicalDeviceProperties2", physical_properties2),
    ENTRY("vkGetPhysicalDeviceMemoryProperties", physical_memory),
    ENTRY("vkGetPhysicalDeviceQueueFamilyProperties", physical_queues),
    ENTRY("vkGetPhysicalDeviceFeatures", physical_features),
    ENTRY("vkGetPhysicalDeviceFormatProperties", physical_format),
    ENTRY("vkGetPhysicalDeviceImageFormatProperties", physical_image_format),
    ENTRY("vkGetPhysicalDeviceSparseImageFormatProperties", physical_sparse),
    ENTRY("vkEnumerateDeviceExtensionProperties", device_extensions),
    ENTRY("vkCreateDevice", create_device),
};

static PFN_vkVoidFunction find(const struct entry *entries, size_t count, const char *name)
{
    for (size_t i = 0; i < count; ++i)
        if (strcmp(entries[i].name, name) == 0)
            return entries[i].function;
    return NULL;
}

static PFN_vkVoidFunction device_function(const char *name)
{
    return find(device_entries, sizeof(device_entries) / sizeof(*device_entries), name);
}

EXPORT VKAPI_ATTR VkResult VKAPI_CALL vk_icdNegotiateLoaderICDInterfaceVersion(uint32_t *version)
{
    if (*version < 5)
        return VK_ERROR_INCOMPATIBLE_DRIVER;
    *version = 5;
    return VK_SUCCESS;
}

EXPORT VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vk_icdGetInstanceProcAddr(VkInstance instance,
                                                                          const char *name)
{
    (void)instance;
    PFN_vkVoidFunction function =
        find(instance_entries, sizeof(instance_entries) / sizeof(*instance_entries), name);
    return function != NULL ? function : device_function(name);
}

EXPORT VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vk_icdGetPhysicalDeviceProcAddr(VkInstance instance,
                                                                                const char *name)
{
    (void)instance;
    (void)name;
    return NULL;
}
