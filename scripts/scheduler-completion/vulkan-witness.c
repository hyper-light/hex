/* AC-T7.3.1: standalone ROOT workload, never a production driver or GPU picker.
 * Vulkan 1.1 deviceUUID and native PCI/LUID are queried independently.
 * Caller supplies every work/count/wait budget. Retain compiler, source, shader
 * and binary hashes. max-items bounds every enumerated Vulkan array.
 */
#include <vulkan/vulkan.h>
#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef __APPLE__
#include <objc/message.h>
#include <objc/runtime.h>
#endif

struct inputs {
    const char *mode, *shader;
    uint8_t uuid[VK_UUID_SIZE];
    uint32_t max_items, words, rounds, challenge;
    uint64_t wait_ns, max_shader_bytes;
};

struct owned {
    VkInstance instance;
    VkDevice device;
    VkBuffer buffer;
    VkDeviceMemory memory;
    void *mapped;
    VkShaderModule shader;
    VkDescriptorSetLayout descriptors;
    VkPipelineLayout layout;
    VkPipeline pipeline;
    VkDescriptorPool pool;
    VkCommandPool commands;
    VkFence fence;
    int submitted, complete;
};

static int fail(const char *phase, int code) {
    fprintf(stderr, "vulkan_witness_failure phase=%s code=%d\n", phase, code);
    return 1;
}

static int number(const char *text, uint64_t *out) {
    if (!text || !*text || *text == '-' || *text == '+') return 1;
    char *end = NULL;
    errno = 0;
    unsigned long long value = strtoull(text, &end, 10);
    if (errno || !end || *end) return 1;
    *out = (uint64_t)value;
    return 0;
}

static int hex_digit(char value) {
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    if (value >= 'A' && value <= 'F') return value - 'A' + 10;
    return -1;
}

static int parse(int argc, char **argv, struct inputs *out) {
    memset(out, 0, sizeof(*out));
    uint32_t seen = 0;
    for (int index = 1; index < argc; index += 2) {
        if (index + 1 >= argc) return fail("missing_argument", 0);
        const char *flag = argv[index], *value = argv[index + 1];
        uint32_t bit = 0;
        uint64_t parsed = 0;
        if (!strcmp(flag, "--mode")) { bit = 1; out->mode = value; }
        else if (!strcmp(flag, "--uuid")) {
            bit = 2;
            if (strlen(value) != VK_UUID_SIZE * 2) return fail("uuid_length", 0);
            for (uint32_t i = 0; i < VK_UUID_SIZE; ++i) {
                int high = hex_digit(value[2 * i]), low = hex_digit(value[2 * i + 1]);
                if (high < 0 || low < 0) return fail("uuid_hex", 0);
                out->uuid[i] = (uint8_t)((high << 4) | low);
            }
        } else if (!strcmp(flag, "--shader")) { bit = 4; out->shader = value; }
        else {
            if (number(value, &parsed)) return fail("decimal_argument", 0);
            if (!strcmp(flag, "--wait-ns")) { bit = 8; out->wait_ns = parsed; }
            else if (!strcmp(flag, "--max-shader-bytes")) { bit = 16; out->max_shader_bytes = parsed; }
            else {
                if (parsed > UINT32_MAX) return fail("u32_argument", 0);
                if (!strcmp(flag, "--max-items")) { bit = 32; out->max_items = (uint32_t)parsed; }
                else if (!strcmp(flag, "--words")) { bit = 64; out->words = (uint32_t)parsed; }
                else if (!strcmp(flag, "--rounds")) { bit = 128; out->rounds = (uint32_t)parsed; }
                else if (!strcmp(flag, "--challenge")) { bit = 256; out->challenge = (uint32_t)parsed; }
                else return fail("unknown_argument", 0);
            }
        }
        if (seen & bit) return fail("duplicate_argument", 0);
        seen |= bit;
    }
    if (!out->mode || !out->max_items) return fail("required_inventory_arguments", 0);
    if (!strcmp(out->mode, "inventory")) return 0;
    if (strcmp(out->mode, "compute") && strcmp(out->mode, "host-compute") && strcmp(out->mode, "reject")) return fail("mode", 0);
    if (!(seen & 2)) return fail("required_uuid", 0);
    if (!strcmp(out->mode, "reject")) return 0;
    if (!out->words || !out->rounds || !out->wait_ns || out->wait_ns == UINT64_MAX ||
        !out->max_shader_bytes || !out->shader || !(seen & 256)) return fail("required_work_arguments", 0);
    return 0;
}

static void hex(const uint8_t *bytes, size_t length) {
    for (size_t i = 0; i < length; ++i) printf("%02x", bytes[i]);
}

static int extension(VkPhysicalDevice physical, const char *name, uint32_t budget) {
    uint32_t count = 0;
    VkResult result = vkEnumerateDeviceExtensionProperties(physical, NULL, &count, NULL);
    if (result != VK_SUCCESS || count > budget) return -1;
    VkExtensionProperties *rows = calloc(count ? count : 1, sizeof(*rows));
    if (!rows) return -1;
    uint32_t available = count;
    result = vkEnumerateDeviceExtensionProperties(physical, NULL, &available, rows);
    int found = 0;
    if (result != VK_SUCCESS || available != count) found = -1;
    else for (uint32_t i = 0; i < count; ++i) if (!strcmp(rows[i].extensionName, name)) found = 1;
    free(rows);
    return found;
}

static int inventory(const struct inputs *args, struct owned *own, VkPhysicalDevice *chosen,
                     VkPhysicalDeviceProperties *properties) {
    VkApplicationInfo app = { .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO, .apiVersion = VK_API_VERSION_1_1 };
    VkInstanceCreateInfo config = { .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo = &app };
#ifdef __APPLE__
    const char *portability = VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME;
    config.flags = VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR;
    config.enabledExtensionCount = 1;
    config.ppEnabledExtensionNames = &portability;
#endif
    VkResult result = vkCreateInstance(&config, NULL, &own->instance);
    if (result != VK_SUCCESS) return fail("create_instance", result);
    uint32_t count = 0;
    result = vkEnumeratePhysicalDevices(own->instance, &count, NULL);
    if (result != VK_SUCCESS || count == 0 || count > args->max_items) return fail("device_count", result);
    VkPhysicalDevice *devices = calloc(count, sizeof(*devices));
    if (!devices) return fail("device_allocation", 0);
    uint32_t actual = count;
    result = vkEnumeratePhysicalDevices(own->instance, &actual, devices);
    if (result != VK_SUCCESS || actual != count) { free(devices); return fail("device_enumeration_changed", result); }
    uint32_t matches = 0;
    if (!strcmp(args->mode, "inventory")) printf("{\"devices\":[");
    for (uint32_t i = 0; i < count; ++i) {
        VkPhysicalDeviceIDProperties id = { .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES };
        VkPhysicalDevicePCIBusInfoPropertiesEXT pci = { .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PCI_BUS_INFO_PROPERTIES_EXT };
        VkPhysicalDeviceProperties2 row = { .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2, .pNext = &id };
        int has_pci = extension(devices[i], VK_EXT_PCI_BUS_INFO_EXTENSION_NAME, args->max_items);
        if (has_pci < 0) { free(devices); return fail("extension_inventory_budget", 0); }
        if (has_pci) id.pNext = &pci;
        vkGetPhysicalDeviceProperties2(devices[i], &row);
        if (!memcmp(args->uuid, id.deviceUUID, VK_UUID_SIZE)) {
            ++matches; *chosen = devices[i]; *properties = row.properties;
        }
        if (!strcmp(args->mode, "inventory")) {
            printf("%s{\"uuid\":\"", i ? "," : ""); hex(id.deviceUUID, VK_UUID_SIZE);
            printf("\",\"device_type\":%u,\"pci\":", row.properties.deviceType);
            if (has_pci) printf("[%" PRIu32 ",%" PRIu32 ",%" PRIu32 ",%" PRIu32 "]", pci.pciDomain, pci.pciBus, pci.pciDevice, pci.pciFunction);
            else printf("null");
            printf(",\"luid\":");
            if (id.deviceLUIDValid) { printf("\""); hex(id.deviceLUID, VK_LUID_SIZE); printf("\""); }
            else printf("null");
            printf(",\"node_mask\":%u", id.deviceLUIDValid ? id.deviceNodeMask : 0);
#ifdef __APPLE__
            /* MoltenVK's official borrowed-MTLDevice API; native registryID is
             * joined directly, never inferred from Vulkan's device name/LUID. */
            typedef void (VKAPI_PTR *metal_device_fn)(VkPhysicalDevice, void **);
            metal_device_fn get_metal = (metal_device_fn)vkGetInstanceProcAddr(own->instance, "vkGetMTLDeviceMVK");
            void *metal = NULL;
            if (!get_metal) { free(devices); return fail("native_metal_join_api", 0); }
            get_metal(devices[i], &metal);
            if (!metal) { free(devices); return fail("native_metal_join_device", 0); }
            uint64_t registry = ((uint64_t (*)(void *, SEL))objc_msgSend)(metal, sel_registerName("registryID"));
            printf(",\"metal_registry_id\":%" PRIu64, registry);
#endif
            printf("}\n");
        }
    }
    free(devices);
    if (!strcmp(args->mode, "inventory")) { printf("]}\n"); return 0; }
    if (!strcmp(args->mode, "reject")) {
        if (matches) return fail("wrong_id_visible", 0);
        printf("{\"refused\":true,\"dispatches\":0}\n"); return 0;
    }
    if (matches != 1 || (!strcmp(args->mode, "compute") && count != 1)) return fail("selected_or_foreign_identity", 0);
    if (properties->deviceType == VK_PHYSICAL_DEVICE_TYPE_CPU ||
        properties->deviceType == VK_PHYSICAL_DEVICE_TYPE_OTHER) return fail("software_or_unknown_device", 0);
    if (properties->apiVersion < VK_API_VERSION_1_1) return fail("device_version", 0);
    return 0;
}

static uint32_t oracle(uint32_t index, uint32_t challenge, uint32_t rounds) {
    uint32_t value = index ^ challenge;
    for (uint32_t round = 0; round < rounds; ++round)
        value = ((value << 7) | (value >> 25)) ^ (index + round + 1);
    return value;
}

static int compute(const struct inputs *args, struct owned *own, VkPhysicalDevice physical,
                   const VkPhysicalDeviceProperties *properties) {
    VkResult result;
#define VK_DO(phase, call) do { result = (call); if (result != VK_SUCCESS) return fail(phase, result); } while (0)
    if (args->words > properties->limits.maxComputeWorkGroupCount[0] ||
        (uint64_t)args->words * sizeof(uint32_t) > properties->limits.maxStorageBufferRange ||
        (uint64_t)args->words > SIZE_MAX / sizeof(uint32_t)) return fail("work_exceeds_device_limits", 0);
    uint32_t queue_count = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(physical, &queue_count, NULL);
    if (!queue_count || queue_count > args->max_items) return fail("queue_budget", 0);
    VkQueueFamilyProperties *queues = calloc(queue_count, sizeof(*queues));
    if (!queues) return fail("queue_allocation", 0);
    uint32_t actual = queue_count;
    vkGetPhysicalDeviceQueueFamilyProperties(physical, &actual, queues);
    uint32_t family = UINT32_MAX;
    for (uint32_t i = 0; i < actual && i < queue_count; ++i)
        if (queues[i].queueCount && (queues[i].queueFlags & VK_QUEUE_COMPUTE_BIT)) { family = i; break; }
    free(queues);
    if (actual != queue_count || family == UINT32_MAX) return fail("compute_queue", 0);
    float priority = 1.0f;
    VkDeviceQueueCreateInfo queue_info = { .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = family, .queueCount = 1, .pQueuePriorities = &priority };
    VkDeviceCreateInfo device_info = { .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .queueCreateInfoCount = 1, .pQueueCreateInfos = &queue_info };
#ifdef __APPLE__
    const char *subset = "VK_KHR_portability_subset";
    device_info.enabledExtensionCount = 1;
    device_info.ppEnabledExtensionNames = &subset;
#endif
    VK_DO("create_device", vkCreateDevice(physical, &device_info, NULL, &own->device));
    VkQueue queue;
    vkGetDeviceQueue(own->device, family, 0, &queue);
    VkDeviceSize bytes = (VkDeviceSize)args->words * sizeof(uint32_t);
    VkBufferCreateInfo buffer = { .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, .size = bytes,
        .usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT, .sharingMode = VK_SHARING_MODE_EXCLUSIVE };
    VK_DO("create_buffer", vkCreateBuffer(own->device, &buffer, NULL, &own->buffer));
    VkMemoryRequirements requirements;
    vkGetBufferMemoryRequirements(own->device, own->buffer, &requirements);
    VkPhysicalDeviceMemoryProperties memories;
    vkGetPhysicalDeviceMemoryProperties(physical, &memories);
    uint32_t memory = UINT32_MAX;
    for (uint32_t i = 0; i < memories.memoryTypeCount && i < VK_MAX_MEMORY_TYPES; ++i) {
        VkMemoryPropertyFlags flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        if ((requirements.memoryTypeBits & (UINT32_C(1) << i)) && (memories.memoryTypes[i].propertyFlags & flags) == flags) { memory = i; break; }
    }
    if (memory == UINT32_MAX) return fail("host_coherent_memory", 0);
    VkMemoryAllocateInfo allocation = { .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = requirements.size, .memoryTypeIndex = memory };
    VK_DO("allocate_memory", vkAllocateMemory(own->device, &allocation, NULL, &own->memory));
    VK_DO("bind_memory", vkBindBufferMemory(own->device, own->buffer, own->memory, 0));
    VK_DO("map_memory", vkMapMemory(own->device, own->memory, 0, bytes, 0, &own->mapped));
    uint32_t *words = own->mapped;
    for (uint32_t i = 0; i < args->words; ++i) words[i] = oracle(i, args->challenge, args->rounds) ^ 1;
    FILE *source = fopen(args->shader, "rb");
    if (!source) return fail("shader_open", errno);
    if (fseek(source, 0, SEEK_END)) { fclose(source); return fail("shader_seek", errno); }
    long extent = ftell(source);
    if (extent <= 0 || (uint64_t)extent > args->max_shader_bytes || (uint64_t)extent > SIZE_MAX || extent % 4) {
        fclose(source); return fail("shader_extent", 0);
    }
    if (fseek(source, 0, SEEK_SET)) { fclose(source); return fail("shader_rewind", errno); }
    uint32_t *code = malloc((size_t)extent);
    if (!code) { fclose(source); return fail("shader_allocation", 0); }
    size_t read = fread(code, 1, (size_t)extent, source);
    int closed = fclose(source);
    if (read != (size_t)extent || closed || code[0] != UINT32_C(0x07230203)) { free(code); return fail("shader_read", 0); }
    VkShaderModuleCreateInfo shader = { .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = (size_t)extent, .pCode = code };
    result = vkCreateShaderModule(own->device, &shader, NULL, &own->shader);
    free(code);
    if (result != VK_SUCCESS) return fail("shader_module", result);
    VkDescriptorSetLayoutBinding binding = { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
        .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT };
    VkDescriptorSetLayoutCreateInfo descriptors = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 1, .pBindings = &binding };
    VK_DO("descriptor_layout", vkCreateDescriptorSetLayout(own->device, &descriptors, NULL, &own->descriptors));
    VkPushConstantRange push = { .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT, .size = 2 * sizeof(uint32_t) };
    VkPipelineLayoutCreateInfo layout = { .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1, .pSetLayouts = &own->descriptors, .pushConstantRangeCount = 1, .pPushConstantRanges = &push };
    VK_DO("pipeline_layout", vkCreatePipelineLayout(own->device, &layout, NULL, &own->layout));
    VkComputePipelineCreateInfo pipeline = { .sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
        .stage = { .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                   .stage = VK_SHADER_STAGE_COMPUTE_BIT, .module = own->shader, .pName = "main" }, .layout = own->layout };
    VK_DO("compute_pipeline", vkCreateComputePipelines(own->device, VK_NULL_HANDLE, 1, &pipeline, NULL, &own->pipeline));
    VkDescriptorPoolSize size = { .type = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1 };
    VkDescriptorPoolCreateInfo pool = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
        .maxSets = 1, .poolSizeCount = 1, .pPoolSizes = &size };
    VK_DO("descriptor_pool", vkCreateDescriptorPool(own->device, &pool, NULL, &own->pool));
    VkDescriptorSetAllocateInfo sets = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
        .descriptorPool = own->pool, .descriptorSetCount = 1, .pSetLayouts = &own->descriptors };
    VkDescriptorSet set;
    VK_DO("descriptor_set", vkAllocateDescriptorSets(own->device, &sets, &set));
    VkDescriptorBufferInfo buffer_info = { .buffer = own->buffer, .range = bytes };
    VkWriteDescriptorSet write = { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = set, .descriptorCount = 1, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .pBufferInfo = &buffer_info };
    vkUpdateDescriptorSets(own->device, 1, &write, 0, NULL);
    VkCommandPoolCreateInfo commands = { .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO, .queueFamilyIndex = family };
    VK_DO("command_pool", vkCreateCommandPool(own->device, &commands, NULL, &own->commands));
    VkCommandBufferAllocateInfo command_info = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = own->commands, .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1 };
    VkCommandBuffer command;
    VK_DO("command_buffer", vkAllocateCommandBuffers(own->device, &command_info, &command));
    VkCommandBufferBeginInfo begin = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT };
    VK_DO("begin_commands", vkBeginCommandBuffer(command, &begin));
    vkCmdBindPipeline(command, VK_PIPELINE_BIND_POINT_COMPUTE, own->pipeline);
    vkCmdBindDescriptorSets(command, VK_PIPELINE_BIND_POINT_COMPUTE, own->layout, 0, 1, &set, 0, NULL);
    uint32_t inputs[] = { args->challenge, args->rounds };
    vkCmdPushConstants(command, own->layout, VK_SHADER_STAGE_COMPUTE_BIT, 0, sizeof(inputs), inputs);
    vkCmdDispatch(command, args->words, 1, 1);
    VkMemoryBarrier barrier = { .sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT, .dstAccessMask = VK_ACCESS_HOST_READ_BIT };
    vkCmdPipelineBarrier(command, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_PIPELINE_STAGE_HOST_BIT, 0, 1, &barrier, 0, NULL, 0, NULL);
    VK_DO("end_commands", vkEndCommandBuffer(command));
    VkFenceCreateInfo fence = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
    VK_DO("create_fence", vkCreateFence(own->device, &fence, NULL, &own->fence));
    VkSubmitInfo submit = { .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO, .commandBufferCount = 1, .pCommandBuffers = &command };
    VK_DO("submit", vkQueueSubmit(queue, 1, &submit, own->fence));
    own->submitted = 1;
    printf("{\"event\":\"dispatch_started\"}\n");
    fflush(stdout);
    VK_DO("finite_fence_wait", vkWaitForFences(own->device, 1, &own->fence, VK_TRUE, args->wait_ns));
    own->complete = 1;
    uint32_t mismatches = 0;
    for (uint32_t i = 0; i < args->words; ++i) if (words[i] != oracle(i, args->challenge, args->rounds)) ++mismatches;
    if (mismatches) return fail("GPU_oracle_mismatch", (int)(mismatches > INT_MAX ? INT_MAX : mismatches));
    printf("{\"uuid\":\""); hex(args->uuid, VK_UUID_SIZE);
    printf("\",\"words\":%u,\"rounds\":%u,\"challenge\":%u,\"mismatches\":0,\"prefill_mismatches\":%u,\"hardware\":true,\"dispatch_completed\":true}\n",
           args->words, args->rounds, args->challenge, args->words);
    return 0;
#undef VK_DO
}

static void cleanup(struct owned *own) {
    if (own->submitted && !own->complete) {
        /* Never free GPU-visible memory still in flight. Only this disposable
         * ROOT witness exits; native/driver cleanup remains unverified. */
        fprintf(stderr, "native_cleanup_uncertain submitted=true completed=false\n");
        fflush(NULL);
        _Exit(1);
    }
    if (own->mapped) vkUnmapMemory(own->device, own->memory);
    if (own->fence) vkDestroyFence(own->device, own->fence, NULL);
    if (own->commands) vkDestroyCommandPool(own->device, own->commands, NULL);
    if (own->pool) vkDestroyDescriptorPool(own->device, own->pool, NULL);
    if (own->pipeline) vkDestroyPipeline(own->device, own->pipeline, NULL);
    if (own->layout) vkDestroyPipelineLayout(own->device, own->layout, NULL);
    if (own->descriptors) vkDestroyDescriptorSetLayout(own->device, own->descriptors, NULL);
    if (own->shader) vkDestroyShaderModule(own->device, own->shader, NULL);
    if (own->buffer) vkDestroyBuffer(own->device, own->buffer, NULL);
    if (own->memory) vkFreeMemory(own->device, own->memory, NULL);
    if (own->device) vkDestroyDevice(own->device, NULL);
    if (own->instance) vkDestroyInstance(own->instance, NULL);
}

int main(int argc, char **argv) {
    struct inputs args;
    if (parse(argc, argv, &args)) return 1;
    struct owned own = {0};
    VkPhysicalDevice physical = VK_NULL_HANDLE;
    VkPhysicalDeviceProperties properties = {0};
    int result = inventory(&args, &own, &physical, &properties);
    if (!result && (!strcmp(args.mode, "compute") || !strcmp(args.mode, "host-compute")))
        result = compute(&args, &own, physical, &properties);
    cleanup(&own);
    return result;
}
