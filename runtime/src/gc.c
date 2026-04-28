/// The map for a single function's stack frame.  One of these is
///        compiled as constant data into the executable for each function.
///
/// Storage of metadata values is elided if the %metadata parameter to
/// @llvm.gcroot is null.
#include "gc.h"
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

struct StackEntry *llvm_gc_root_chain;
Object *heap = NULL;

void **worklist;
size_t top;

void visitGCRoots(void (*Visitor)(void **root, const void *meta)) {
  for (struct StackEntry *R = llvm_gc_root_chain; R; R = R->next) {
    unsigned i = 0;

    // For roots [0, NumMeta), the metadata pointer is in the FrameMap.
    for (unsigned e = R->map->numMeta; i != e; ++i)
      Visitor(&R->roots[i], R->map->meta[i]);

    // For roots [NumMeta, NumRoots), the metadata pointer is null.
    for (unsigned e = R->map->numRoots; i != e; ++i)
      Visitor(&R->roots[i], NULL);
  }
}

// Todo - implement my own allocator
void *gc_new(size_t size) {
  Object *object = malloc(sizeof(Object) + size);
  if (object == NULL) {
    collect();
    object = malloc(sizeof(Object) + size);
    if (object == NULL) {
      fprintf(stderr, "Out of memory");
      return NULL;
    }
  }
  object->isMarked = 0;
  object->next = heap;
  heap = object;
  return (void *)(object + 1);
}

// Todo - malloc and realloc to initialize and extend worklist respectively
void collect() {
  worklist = NULL;
  top = 0;
  visitGCRoots(markFromRoots);
  sweep();
}

void markFromRoots(void **root, const void *meta) {
  void *ref = *root;
  if (ref != NULL && !isMarked(ref)) {
    setMarked(ref);
    add(ref, meta);
    mark();
  }
}

void mark() {
  while (!isEmpty()) {
    ObjectInfo *object = del();
    Pointers *ptrs = pointers(object);
    size_t size = ptrs->size;
    for (size_t i = 0; i < size; i++) {
      void *child = ptrs->ptrs[i];
      if (child != NULL && !isMarked(child)) {
        setMarked(child);
        add(child, NULL);
      }
    }
  }
}

// Todo - remove object from the heap
void sweep() {
  for (Object *object = heap; object; object = object->next) {
    if (object->isMarked) {
      object->isMarked = 0;
    } else {
      free(object);
    }
  }
}

int isMarked(void *ptr) {
  Object *object = extractHeader(ptr);
  return object->isMarked;
}

void setMarked(void *ptr) {
  Object *object = extractHeader(ptr);
  object->isMarked = 1;
}

Object *extractHeader(void *ptr) { return ((Object *)ptr) - 1; }

int isEmpty() { return top == 0; }

ObjectInfo *del() { return worklist[--top]; }

void add(void *ptr, const void *meta) {
  MetaData *meta_data = (MetaData *)meta;
  ObjectInfo *object_info = malloc(sizeof(ObjectInfo));
  object_info->ptr = ptr;
  object_info->meta_data = meta_data;
  worklist[top++] = object_info;
}

Pointers *pointers(ObjectInfo *object_info) {
  MetaData *meta_data = object_info->meta_data;
  Pointers *pointers = NULL;
  if (!meta_data) {
    return pointers;
  }
  size_t size = meta_data->numPtrs;
  void **ptrs = malloc(sizeof(void *) * size);
  for (size_t i = 0; i < size; i++) {
    ptrs[i] = (void *)((char *)object_info->ptr + meta_data->fields[i]);
  }
  pointers = malloc(sizeof(Pointers));
  pointers->ptrs = ptrs;
  pointers->size = size;
  return pointers;
}
