#ifndef GC_H
#define GC_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
struct FrameMap {
  int32_t num_roots; //< Number of roots in stack frame.
};

/// A link in the dynamic shadow stack.  One of these is embedded in
///        the stack frame of each function on the call stack.
struct StackEntry {
  struct StackEntry *next;    //< Link to next stack entry (the caller's).
  const struct FrameMap *map; //< Pointer to constant FrameMap.
  void *roots[0];             //< Stack roots (in-place array).
};

typedef struct Object {
  int32_t is_marked;
  int32_t is_pointer;
} Object;

typedef struct ConsCell {
  struct ConsCell *cdr;
} ConsCell;

typedef struct Allocator {
  void *start;
  size_t size;
  ConsCell *f;
} Allocator;

/// The head of the singly-linked list of StackEntries.  Functions push
///        and pop onto this in their prologue and epilogue.
///
/// Since there is only a global list, this technique is not threadsafe.
extern struct StackEntry *llvm_gc_root_chain;
extern struct Allocator *allocator;

/// Calls Visitor(root, meta) for each GC root on the stack.
///        root and meta are exactly the values passed to
///        @llvm.gcroot.
///
/// Visitor could be a function to recursively mark live objects.  Or it
/// might copy them to another heap or generation.
///
/// @param Visitor A function to invoke for every GC root on the stack.
///
void visitGCRoots(void (*visitor)(void **root));
struct StackEntry *get_gc_root_chain();
void gc_pop();
void gc_push(struct StackEntry *se);
void collect();
void *gc_new(int32_t is_pointer);
void markFromRoots(void **root);
void mark(void *ptr);
void sweep();
void setMarked(void *ptr);
int isMarked(void *ptr);
Object *extractHeader(void *ptr);
int initAllocator();
#endif
