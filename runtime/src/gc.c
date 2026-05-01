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

#define ALLOCATOR_SIZE (100 * 1024 * 1024)
#define CELL_SIZE 18

struct StackEntry *llvm_gc_root_chain;

Allocator *allocator;

void **worklist;

size_t top;

void init_allocator() {
  allocator = malloc(sizeof(Allocator));
  allocator->start = malloc(ALLOCATOR_SIZE);
  allocator->size = 100000;
  size_t size = ALLOCATOR_SIZE / (CELL_SIZE);
  char *ptr = (char *)allocator->start;
  ConsCell *prev = NULL;

  for (size_t i = 0; i < size; i++) {
    ConsCell *curr = (ConsCell *)(10 + ptr);
    if (prev) {
      prev->cdr = curr;
    } else {
      allocator->F = curr;
    }
    prev = curr;
    ptr += (CELL_SIZE);
  }
  if (prev) {
    prev->cdr = NULL;
  }
}

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

void *gc_new(int isPointer) {
  if (allocator->F == NULL) {
    collect();
    if (allocator->F == NULL) {
      fprintf(stderr, "Out of memory");
      return NULL;
    }
  }
  Object *object = (Object *)((char *)allocator->F - 10);
  object->isMarked = 0;
  object->isPointer = isPointer;
  allocator->F = allocator->F->cdr;
  return (void *)(object + 1);
}

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
    mark(ref);
  }
}

void mark(void *ptr) {

  Object *object = (Object *)((char *)ptr - 10);
  void *car = object->isPointer ? *(void **)ptr : NULL;
  if (car != NULL && !isMarked(car)) {
    setMarked(car);
    add(car, NULL);
  }
  ConsCell *cdr = (ConsCell *)((char *)ptr + 8);
  ConsCell *next_cdr = cdr->cdr;
  if (next_cdr != NULL) {
    void *next_cell = (char *)next_cdr - 8;
    if (!isMarked(next_cell)) {
      setMarked(next_cell);
      add(next_cell, NULL);
    }
  }
}

void sweep() {
  size_t size = ALLOCATOR_SIZE / (CELL_SIZE);
  allocator->F = NULL;
  ConsCell *prev = NULL;
  char *ptr = (char *)allocator->start;
  for (int i = 0; i < size; i++) {
    Object *object = (Object *)ptr;
    if (object->isMarked) {
      object->isMarked = 0;
    } else {
      ConsCell *curr = (ConsCell *)(ptr + 10);
      if (!allocator->F) {
        allocator->F = curr;
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
  return object->isMarked;
}

void setMarked(void *ptr) {
  Object *object = extractHeader(ptr);
  object->isMarked = 1;
}

Object *extractHeader(void *ptr) { return ((Object *)ptr) - 1; }

int isEmpty() { return top == 0; }

ObjectInfo *del() { return worklist[--top]; }
