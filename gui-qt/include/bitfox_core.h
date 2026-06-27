#ifndef BITFOX_CORE_H
#define BITFOX_CORE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct CcBoard CcBoard;

typedef struct {
    int legal;
    int capture;
    int castle;
    int ep;
    int promo;
    int check;
    int status;
} CcMoveInfo;

typedef struct {
    uint32_t best;
    int score;
    int depth;
    int seldepth;
    uint64_t nodes;
    int pv_len;
    uint32_t pv[64];
} CcSearchResult;

const char *cc_version(void);
CcBoard *cc_new(void);
void cc_free(CcBoard *board);
int cc_set_fen(CcBoard *board, const char *fen);
int cc_get_fen(CcBoard *board, char *out, int size);
int cc_legal_to(CcBoard *board, int from, int *out);
int cc_premove_to(CcBoard *board, int from, int *out);
int cc_apply(CcBoard *board, int from, int to, int promo, CcMoveInfo *info);
int cc_piece_at(CcBoard *board, int square);
int cc_side(CcBoard *board);
int cc_in_check(CcBoard *board);
int cc_status(CcBoard *board);
int cc_evaluate(CcBoard *board);
void cc_search(CcBoard *board, int max_depth, int movetime_ms, CcSearchResult *out);

#ifdef __cplusplus
}
#endif

#endif
