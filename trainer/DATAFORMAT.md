# Training data format

`bitfox datagen` writes plain text, one position per line:

```
<FEN> | <score> | <wdl>
```

- **FEN** - the position. Only the board and side-to-move fields are used.
- **score** - the engine's search score in centipawns, **white-relative**
  (positive favours white), clamped to a sane bound. Mate-range scores are
  dropped during generation.
- **wdl** - the game's eventual result, white-relative: `1.0` white win,
  `0.5` draw, `0.0` white loss.

Only quiet positions are emitted (side to move not in check, best move not a
capture or promotion), which keeps the eval target stable.

`convert/` parses this with `bulletformat`'s reader and writes the binary
`ChessBoard` records that Bullet streams during training, so the trainer never
sees the text directly. The trainer blends `score` and `wdl` into the regression
target through its WDL scheduler.
