# Training data format

`bitfox datagen` writes plain text, one position per line:

```
<FEN> | <score> | <wdl>
```

- **FEN** - the position. Only the board and side-to-move fields are used.
- **score** - the engine's search score in centipawns, **white-relative**
  (positive favours white). Mate-range scores are dropped during generation
  rather than clamped, so the column never carries a distance-to-mate.
- **wdl** - how the game finished, white-relative: `1.0` white win, `0.5` draw,
  `0.0` white loss. A game that is already decided is adjudicated rather than
  played out - once one side has held 2000cp or better for five plies in a row
  it is recorded as a win for that side - so this is the result the game was
  headed for, not necessarily one that was reached on the board.

Only quiet positions are emitted (side to move not in check, best move not a
capture or promotion), which keeps the eval target stable.

`convert/` parses this with `bulletformat`'s reader and writes the binary
`ChessBoard` records that Bullet streams during training, so the trainer never
sees the text directly. The trainer blends `score` and `wdl` into the regression
target through its WDL scheduler.
