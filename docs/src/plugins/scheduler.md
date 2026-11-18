# Scheduler

`server.scheduler` runs code on the game tick. There are **20 ticks a second**, so `20` is one
second and `1200` is one minute.

## Run once, later

```rust
server.scheduler.run_later(100, |server| {
    server.broadcast("Five seconds have passed.");
});
```

## Run on a timer

`run_repeating(delay, period, f)` waits `delay` ticks, then runs `f` every `period` ticks.
Return `false` from `f` to stop.

```rust
fn on_server_start(&self, server: &Server) {
    server.scheduler.run_repeating(20 * 60, 20 * 60 * 10, |server| {
        server.broadcast("§7Remember to vote for the server!");
        true // keep going
    });
}
```

## Cancel

Both calls return an id:

```rust
let id = server.scheduler.run_repeating(20, 20, |_| true);
// later
server.scheduler.cancel(id);
```

## Keep tasks short

Tasks run one after another on the tick thread. Something slow, like a web request or a big
file write, holds up the whole tick. Do slow work on a separate thread and only touch the
server from the task.

Tasks need to be `Send + 'static`, so move what they need into the closure (`move |server| ...`)
and use `Arc` for anything shared.
