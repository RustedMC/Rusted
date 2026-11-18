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

## Slow work

Tasks run one after another on the tick thread, so something slow, like a web request or a
database query, holds up the whole server. Use `run_async` for that. The first closure runs on
its own thread. The second gets the result back on the tick, where it's safe to touch players
and the world:

```rust
let name = player.name.clone();
let id = player.id;
server.scheduler.run_async(
    move || look_up_rank_online(&name), // own thread: don't touch the server here
    move |server, rank| server.tell(id, &format!("Your rank: {rank}")),
);
```

Tasks need to be `Send + 'static`, so move what they need into the closure (`move |server| ...`)
and use `Arc` for anything shared.
