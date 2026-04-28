#ifndef GC_H
#define GC_H

#include <stddef.h>
#include <stdint.h>
struct FrameMap {
  int32_t numRoots;    //< Number of roots in stack frame.
  int32_t numMeta;     //< Number of metadata entries.  May be < NumRoots.
  const void *meta[0]; //< Metadata for each root.
};

/// A link in the dynamic shadow stack.  One of these is embedded in
///        the stack frame of each function on the call stack.
struct StackEntry {
  struct StackEntry *next;    //< Link to next stack entry (the caller's).
  const struct FrameMap *map; //< Pointer to constant FrameMap.
  void *roots[0];             //< Stack roots (in-place array).
};

typedef struct Object {
  int isMarked;
  struct Object *next;
} Object;

typedef struct ObjectInfo {
  void *ptr;
  struct MetaData *meta_data;
} ObjectInfo;

typedef struct MetaData {
  size_t numPtrs;
  size_t fields[0];
} MetaData;

typedef struct Pointers {
  size_t size;
  void **ptrs;
} Pointers;

/// The head of the singly-linked list of StackEntries.  Functions push
///        and pop onto this in their prologue and epilogue.
///
/// Since there is only a global list, this technique is not threadsafe.
extern struct StackEntry *llvm_gc_root_chain;
extern Object *heap;
extern void **worklist;
extern size_t top;

/// Calls Visitor(root, meta) for each GC root on the stack.
///        root and meta are exactly the values passed to
///        @llvm.gcroot.
///
/// Visitor could be a function to recursively mark live objects.  Or it
/// might copy them to another heap or generation.
///
/// @param Visitor A function to invoke for every GC root on the stack.
///
void visitGCRoots(void (*Visitor)(void **Root, const void *Meta));

void collect();
void *gc_new(size_t size);
void markFromRoots(void **Root, const void *Meta);
void mark();
void sweep();
void setMarked(void *ptr);
int isMarked(void *ptr);
Object *extractHeader(void *ptr);
ObjectInfo *del();
void add(void *ptr, const void *meta);
int isEmpty();
Pointers *pointers(ObjectInfo *object_info);
#endif
