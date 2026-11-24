# Blocks, queues and parallel groups

In -A, work is described by *blocks*. A block is a named body of statements with
a queue assignment; the queue decides *when* it runs.

```
block load(1)   { aout("loading") }
block parse(2)  { aout("parsing") }
block report(3) { aout("reporting") }
```

Queues run in ascending order. Queue 2 starts only after queue 1 has finished;
this is the barrier between queues, implemented with `pthread_join`.

## Queue notation

| Notation | Meaning |
| --- | --- |
| `(N)` | the block runs alone in queue `N` |
| `(N:N)` | this block is one of two blocks that run in parallel in queue `N` |
| `(N:NxK)` | this block is one of `K` (`K >= 2`) parallel blocks in queue `N` |

There is no `x2` shorthand (`(2:2x2)` is rejected in favour of `(2:2)`) and no
`1:1:1:1` form: a group is written once, on every member.

## Validation

The compiler rejects, with `E_QUEUE`:

* a queue number of `0` (`queues start at 1`);
* two blocks claiming the same bare queue: *duplicate assignment to queue 1: 2
  blocks declare queue 1 but only one block may run alone in a queue*;
* `(N:M)` with different numbers: *both numbers must match*;
* a group that declares more members than exist: *missing parallel group
  members in queue 1*;
* a group that declares fewer: *excess parallel group members in queue 1*;
* a group mixed with a lone block in the same queue: *inconsistent parallel
  group in queue 1*.

## Parallel groups

```
block left(1:1)  { aout("left") }
block right(1:1) { aout("right") }
```

Both blocks are started with `pthread_create` and joined before the next queue.
Each thread has its own stack frame, so locals never alias.

## Output order

Printing inside a parallel group is deterministic: every member writes into its
own output buffer, and once the whole group has been joined the buffers are
flushed in **source declaration order**. This program always prints `left` and
then `right`, on every run:

```
block left(1:1)  { aout("left") }
block right(1:1) { aout("right") }
```

The same holds for larger groups and regardless of how long each block takes:

```
block main(1:1x3) { aout("hello world") }
block main(1:1x3) { aout("meow") }
block main(1:1x3) { aout("third") }
```

prints `hello world`, `meow`, `third`, even if the first block is the slowest.
Inside one block the `aout` calls also keep their program order. Blocks are still
started together and still run at the same time; only the *printing* is
serialised, and always by declaration order, never by completion order. The next
queue still starts after the whole group has finished and its output has been
flushed. See [RUNTIME.md](RUNTIME.md#deterministic-parallel-output).

The test suite proves the parallelism is real, not simulated: two CPU-bound
blocks in one parallel group finish in clearly less wall-clock time than the same
two blocks in successive queues, two blocks incrementing a shared `@atomic_add`
counter 20 000 times each always produce exactly 40 000, and two blocks that
publish a flag for the other to observe both see it (which is impossible if the
group ran sequentially).

## Repeated block names

Block names are documentation, not identity. Repeating a name is legal and every
scheduled declaration is an independent execution context:

```
block worker(1) { set x = 1  aout(A, x) }
block worker(2) { set x = 2  aout(A, x) }
block worker(3) { set x = 3  aout(A, x) }
```

prints `1`, `2`, `3`. Nothing is merged and no duplicate-name error is raised.
In the generated assembly the blocks are `__a_block_0`, `__a_block_1`, … in
schedule order.

## Reuse in blocks

A block header may attach reusable logic; the block evaluates it once and binds
the result for the whole body:

```
reuse roll() { return random(1, 6) }

block show(roll, 1) { aout(A, roll) }
block pair(roll(1, 6), 1:1) { … }
```

Attached reuses are evaluated per block execution, which is why two blocks in a
parallel group each get their own value. See [REUSE.md](REUSE.md).
