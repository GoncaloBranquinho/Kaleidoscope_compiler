/// The map for a single function's stack frame.  One of these is
///        compiled as constant data into the executable for each function.
///
/// Storage of metadata values is elided if the %metadata parameter to
/// @llvm.gcroot is null.
#include "gc.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define ALLOCATOR_SIZE (100 * 1024 * 1024)
#define CELL_SIZE 24

struct StackEntry *llvm_gc_root_chain;

Allocator *allocator;

int initAllocator() {
  allocator = malloc(sizeof(Allocator));
  allocator->start = malloc(ALLOCATOR_SIZE);
  allocator->size = 100000;
  size_t size = ALLOCATOR_SIZE / (CELL_SIZE);
  char *ptr = (char *)allocator->start;
  ConsCell *prev = NULL;

  for (size_t i = 0; i < size; i++) {
    ConsCell *curr = (ConsCell *)(16 + ptr);
    if (prev) {
      prev->cdr = curr;
    } else {
      allocator->f = curr;
    }
    prev = curr;
    ptr += (CELL_SIZE);
  }
  if (prev) {
    prev->cdr = NULL;
  }
  return 1;
}

void visitGCRoots(void (*visitor)(void **root, const void *meta)) {
  for (struct StackEntry *r = llvm_gc_root_chain; r; r = r->next) {
    unsigned i = 0;

    // For roots [0, NumMeta), the metadata pointer is in the FrameMap.
    for (unsigned e = r->map->num_meta; i != e; ++i)
      visitor(&r->roots[i], r->map->meta[i]);

    // For roots [NumMeta, NumRoots), the metadata pointer is null.
    for (unsigned e = r->map->num_roots; i != e; ++i)
      visitor(&r->roots[i], NULL);
  }
}

void *gc_new(bool is_pointer) {
  if (allocator->f == NULL) {
    collect();
    if (allocator->f == NULL) {
      fprintf(stderr, "Out of memory");
      return NULL;
    }
  }
  Object *object = (Object *)((char *)allocator->f - 16);
  object->is_marked = 0;
  object->is_pointer = is_pointer;
  allocator->f = allocator->f->cdr;
  return (void *)(object + 1);
}

void collect() {
  visitGCRoots(markFromRoots);
  sweep();
}

void markFromRoots(void **root, const void *meta) {
  void *ref = *root;
  if (ref != NULL && !isMarked(ref)) {
    setMarked(ref);
    mark(ref);
  }
}

void mark(void *ptr) {

  Object *object = (Object *)((char *)ptr - 16);
  void *car = object->is_pointer ? *(void **)ptr : NULL;
  if (car != NULL && !isMarked(car)) {
    setMarked(car);
    mark(car);
  }
  ConsCell *cdr = (ConsCell *)((char *)ptr + 8);
  ConsCell *next_cdr = cdr->cdr;
  if (next_cdr != NULL) {
    void *next_cell = (char *)next_cdr - 8;
    if (!isMarked(next_cell)) {
      setMarked(next_cell);
      mark(next_cell);
    }
  }
}

void sweep() {
  size_t size = ALLOCATOR_SIZE / (CELL_SIZE);
  allocator->f = NULL;
  ConsCell *prev = NULL;
  char *ptr = (char *)allocator->start;
  for (size_t i = 0; i < size; i++) {
    Object *object = (Object *)ptr;
    if (object->is_marked) {
      object->is_marked = 0;
    } else {
      ConsCell *curr = (ConsCell *)(ptr + 16);
      if (!allocator->f) {
        allocator->f = curr;
      } else {
        prev->cdr = curr;
      }
      prev = curr;
    }
    ptr += CELL_SIZE;
  }
  if (prev) {
    prev->cdr = NULL;
  }
}

int isMarked(void *ptr) {
  Object *object = extractHeader(ptr);
  return object->is_marked;
}

void setMarked(void *ptr) {
  Object *object = extractHeader(ptr);
  object->is_marked = 1;
}

Object *extractHeader(void *ptr) { return ((Object *)ptr) - 1; }
