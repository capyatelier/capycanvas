#!/usr/bin/env python3
"""Run GTK's actual Vulkan allocator with mock memory and deterministic timers."""
from pathlib import Path
import re
import subprocess
import sys
import tempfile

root = Path(sys.argv[1]).resolve()
gpu = root / "gsk/gpu"

header = r'''
#include <glib.h>
#include <vulkan/vulkan.h>
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#define MIB (1024ull*1024)
#define GSK_VK_CHECK(fn,...) assert(fn(__VA_ARGS__)==VK_SUCCESS)
static gint64 clock_us=10000000;
#define g_get_monotonic_time() clock_us
static unsigned allocated,freed,maps,unmaps,live,added,removed,calls;
static VkDeviceSize sizes[1000];
static gboolean mapped[1000],destroying;
typedef struct { GSourceFunc callback; gpointer data; gboolean active; } Timer;
static Timer timers[1000];
static unsigned active(void) {
  unsigned n=0;for(unsigned i=1;i<=added;i++)n+=timers[i].active;return n;
}
static guint timeout(guint milliseconds,GSourceFunc cb,gpointer data) {
  assert(milliseconds==100 && cb && data);unsigned id=++added;
  assert(id<G_N_ELEMENTS(timers));timers[id]=(Timer){cb,data,TRUE};return id;
}
static gboolean remove_timer(guint id) {
  assert(id && id<=added && timers[id].active);
  timers[id].active=FALSE;removed++;return TRUE;
}
static void tick(void) {
  unsigned last=added;
  for(unsigned i=1;i<=last;i++)if(timers[i].active) {
    calls++;if(timers[i].callback(timers[i].data)==G_SOURCE_REMOVE)timers[i].active=FALSE;
  }
}
#define g_timeout_add timeout
#define g_source_remove remove_timer
static VkResult allocate(VkDevice d,const VkMemoryAllocateInfo *info,
                         const VkAllocationCallbacks *cb,VkDeviceMemory *out) {
  assert(d && !info->pNext && !cb);*out=(VkDeviceMemory)(uintptr_t)++allocated;
  assert(allocated<G_N_ELEMENTS(sizes));sizes[allocated]=info->allocationSize;live++;return VK_SUCCESS;
}
static void release(VkDevice d,VkDeviceMemory m,const VkAllocationCallbacks *cb) {
  unsigned i=(uintptr_t)m;assert(d && !cb && i && sizes[i] && !mapped[i]);
  if(destroying)assert(!active());sizes[i]=0;freed++;live--;
}
static VkResult map(VkDevice d,VkDeviceMemory m,VkDeviceSize off,VkDeviceSize size,
                    VkMemoryMapFlags flags,void **out) {
  unsigned i=(uintptr_t)m;assert(d && sizes[i]>=size && !off && !flags && !mapped[i]);
  mapped[i]=TRUE;maps++;*out=(void *)(uintptr_t)(0x10000000+i*0x1000000);return VK_SUCCESS;
}
static void unmap(VkDevice d,VkDeviceMemory m) {
  unsigned i=(uintptr_t)m;assert(d && mapped[i]);mapped[i]=FALSE;unmaps++;
}
#define vkAllocateMemory allocate
#define vkFreeMemory release
#define vkMapMemory map
#define vkUnmapMemory unmap
'''

private = (gpu / "gskvulkanmemoryprivate.h").read_text().replace("#include <gdk/gdk.h>", "").replace("#pragma once", "")
source = re.sub(r'^#include "(?:config.h|gskvulkanmemoryprivate.h|gskvulkandeviceprivate.h)"\n', "", (gpu / "gskvulkanmemory.c").read_text(), flags=re.M)
tests = r'''
static GskVulkanAllocator *allocator(VkMemoryPropertyFlags flags) {
  VkMemoryType type={.propertyFlags=flags};
  return gsk_vulkan_buddy_allocator_new(gsk_vulkan_direct_allocator_new((VkDevice)1,0,&type),MIB);
}
static GskVulkanAllocation get(GskVulkanAllocator *a,VkDeviceSize size) {
  GskVulkanAllocation x={0};gsk_vulkan_alloc(a,size,4096,&x);return x;
}
static void finish(GskVulkanAllocator *a) {
  destroying=TRUE;gsk_vulkan_allocator_unref(a);destroying=FALSE;assert(!live && !active());
}
int main(void) {
  GskVulkanAllocator *a=allocator(0);GskVulkanAllocation x=get(a,MIB/2),y=get(a,MIB/2);
  assert(allocated==1 && sizes[(uintptr_t)x.vk_memory]==MIB && x.vk_memory==y.vk_memory && x.offset!=y.offset);
  gsk_vulkan_free(a,&x);gsk_vulkan_free(a,&y);assert(!active());finish(a);
  a=allocator(0);x=get(a,MIB/2+1);
  assert(x.size==MIB/2+1 && sizes[(uintptr_t)x.vk_memory]==MIB/2+MIB/16 && !x.offset);
  gsk_vulkan_free(a,&x);assert(active()==1);
  unsigned before=allocated;VkDeviceMemory first=x.vk_memory;
  y=get(a,MIB/2+MIB/32);
  assert(y.vk_memory==first && y.size==MIB/2+MIB/32 && allocated==before && !active());
  gsk_vulkan_free(a,&y);x=get(a,MIB/2+MIB/16+1);
  assert(x.vk_memory!=first && sizes[(uintptr_t)first] && active()==1);
  gsk_vulkan_free(a,&x);finish(a);
  a=allocator(0);x=get(a,G_MAXUINT64-1);
  assert(x.size==G_MAXUINT64-1 && sizes[(uintptr_t)x.vk_memory]>=x.size);
  gsk_vulkan_free(a,&x);finish(a);
  a=allocator(0);GskVulkanAllocation items[5];
  for(unsigned i=0;i<5;i++)items[i]=get(a,2*MIB);
  before=added;
  for(unsigned i=0;i<5;i++)gsk_vulkan_free(a,&items[i]);
  assert(live==4 && !sizes[(uintptr_t)items[0].vk_memory] && added==before+1 && active()==1);
  for(unsigned i=1;i<5;i++) { x=get(a,2*MIB);assert(x.vk_memory==items[i].vk_memory);items[i]=x; }
  assert(!active());for(unsigned i=1;i<5;i++)gsk_vulkan_free(a,&items[i]);finish(a);
  for(unsigned flags=0;flags<=GSK_VULKAN_MEMORY_MAPPABLE;flags+=VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT) {
    a=allocator(flags);x=get(a,2*MIB+1);first=x.vk_memory;guchar *pointer=x.map;
    assert((pointer!=NULL)==(flags==GSK_VULKAN_MEMORY_MAPPABLE));
    unsigned old_maps=maps,old_unmaps=unmaps,old_freed=freed;
    gsk_vulkan_free(a,&x);y=get(a,2*MIB+2);
    assert(y.vk_memory==first && y.map==pointer && y.size==2*MIB+2 && maps==old_maps && unmaps==old_unmaps && freed==old_freed);
    gsk_vulkan_free(a,&y);clock_us+=100000;tick();
    assert(!live && !active() && unmaps==old_unmaps+(pointer!=NULL));finish(a);
  }
  a=allocator(GSK_VULKAN_MEMORY_MAPPABLE);for(unsigned i=0;i<5;i++)items[i]=get(a,2*MIB);
  before=unmaps;for(unsigned i=0;i<5;i++)gsk_vulkan_free(a,&items[i]);
  assert(unmaps==before+1);finish(a);
  a=allocator(0);x=get(a,2*MIB);y=get(a,3*MIB);clock_us+=10*G_TIME_SPAN_SECOND;
  before=added;gsk_vulkan_free(a,&x);clock_us+=50000;gsk_vulkan_free(a,&y);
  assert(added==before+1);clock_us+=50000;tick();assert(live==1 && active()==1 && added==before+1);
  assert(!sizes[(uintptr_t)x.vk_memory] && sizes[(uintptr_t)y.vk_memory]);
  clock_us+=50000;tick();assert(!live && !active());finish(a);
  a=allocator(0);x=get(a,2*MIB);y=get(a,3*MIB);GskVulkanAllocation z=get(a,4*MIB);
  gsk_vulkan_free(a,&x);clock_us+=25000;gsk_vulkan_free(a,&y);clock_us+=25000;gsk_vulkan_free(a,&z);
  y=get(a,3*MIB);clock_us+=50000;tick();
  assert(!sizes[(uintptr_t)x.vk_memory] && sizes[(uintptr_t)y.vk_memory] && sizes[(uintptr_t)z.vk_memory] && active()==1);
  clock_us+=50000;tick();assert(live==1 && !sizes[(uintptr_t)z.vk_memory] && !active());
  gsk_vulkan_free(a,&y);finish(a);
  a=allocator(GSK_VULKAN_MEMORY_MAPPABLE);x=get(a,2*MIB);y=get(a,3*MIB);
  gsk_vulkan_free(a,&x);clock_us+=100000;before=unmaps;gsk_vulkan_free(a,&y);
  assert(!sizes[(uintptr_t)x.vk_memory] && sizes[(uintptr_t)y.vk_memory] && live==1 && unmaps==before+1);
  before=maps;x=get(a,3*MIB);assert(x.vk_memory==y.vk_memory && maps==before && !active());
  gsk_vulkan_free(a,&x);finish(a);
  a=allocator(0);x=get(a,2*MIB);gsk_vulkan_free(a,&x);before=removed;
  gsk_vulkan_allocator_ref(a);gsk_vulkan_allocator_unref(a);assert(active()==1 && removed==before);
  before=freed;finish(a);assert(freed==before+1);before=calls;tick();assert(calls==before);
  a=gsk_vulkan_external_allocator_new((VkDevice)1);x=get(a,2*MIB);assert(!x.vk_memory && !active());
  VkMemoryAllocateInfo info={.sType=VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,.allocationSize=2*MIB};
  allocate((VkDevice)1,&info,NULL,&x.vk_memory);gsk_vulkan_free(a,&x);finish(a);
  assert(maps==unmaps);puts("PASS: actual GTK buddy/pool rounding, logical sizes, FIFO, mapping, 100ms expiry, shutdown and external memory");
}
'''
with tempfile.TemporaryDirectory(prefix="gtk-vulkan-memory-") as directory:
    c = Path(directory) / "memory.c"
    binary = Path(directory) / "memory"
    c.write_text(header + private + source + tests)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "glib-2.0"], text=True).split()
    subprocess.run(["cc", "-Werror=implicit-function-declaration", "-I", str(root),
                    str(c), "-o", str(binary), *flags], check=True)
    subprocess.run([str(binary)], check=True)
