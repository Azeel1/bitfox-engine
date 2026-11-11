#ifndef BITFOX_CORE_H
#define BITFOX_CORE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct CcBoard CcBoard;
typedef struct CcSearcher CcSearcher;

enum {
    CC_ABI_VERSION = 1,
    CC_MOVE_CAPACITY = 256,
    CC_SQUARE_TARGET_CAPACITY = 32,
    CC_UCI_BUFFER_SIZE = 6,
    CC_PV_CAPACITY = 64,
    CC_DEFAULT_TT_MB = 64,
    CC_MAX_TT_MB = 4096
};

typedef int CcResult;
enum {
    CC_RESULT_OK = 0,
    CC_RESULT_NULL_POINTER = -1,
    CC_RESULT_INVALID_ARGUMENT = -2,
    CC_RESULT_BUFFER_TOO_SMALL = -3,
    CC_RESULT_BUSY = -4,
    CC_RESULT_INTERNAL_ERROR = -5
};

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
    uint32_t pv[CC_PV_CAPACITY];
} CcSearchResult;

typedef struct {
    int material;
    int legal;
    int captures;
    int checks;
    int promotions;
    int in_check;
} CcPositionInfo;

const char *cc_version(void);
uint32_t cc_abi_version(void);
CcBoard *cc_new(void);
void cc_free(CcBoard *board);
void cc_tt_clear(void);
void cc_tt_resize(int mb);
CcResult cc_tt_clear_ex(void);
CcResult cc_tt_resize_ex(int mb);

CcResult cc_searcher_new(int tt_mb, CcSearcher **out);
void cc_searcher_free(CcSearcher *searcher);
CcResult cc_searcher_clear(CcSearcher *searcher);
CcResult cc_searcher_resize(CcSearcher *searcher, int tt_mb);
CcResult cc_searcher_set_threads(CcSearcher *searcher, int threads);
CcResult cc_searcher_stop(CcSearcher *searcher);
CcResult cc_searcher_run(CcSearcher *searcher, CcBoard *board, int max_depth,
                         int movetime_ms, CcSearchResult *out);

int cc_set_fen(CcBoard *board, const char *fen);
int cc_get_fen(CcBoard *board, char *out, int size);
CcResult cc_get_fen_n(CcBoard *board, char *out, size_t capacity,
                      size_t *required);
uint64_t cc_perft(CcBoard *board, int depth);

int cc_gen_legal(CcBoard *board, uint32_t out[CC_MOVE_CAPACITY]);
CcResult cc_gen_legal_n(CcBoard *board, uint32_t *out, size_t capacity,
                        size_t *required);
int cc_legal_to(CcBoard *board, int from, int out[CC_SQUARE_TARGET_CAPACITY]);
CcResult cc_legal_to_n(CcBoard *board, int from, int *out, size_t capacity,
                       size_t *required);
int cc_premove_to(CcBoard *board, int from, int out[CC_SQUARE_TARGET_CAPACITY]);
CcResult cc_premove_to_n(CcBoard *board, int from, int *out, size_t capacity,
                         size_t *required);
int cc_do_move(CcBoard *board, int from, int to, int promo);
int cc_apply(CcBoard *board, int from, int to, int promo, CcMoveInfo *info);
CcResult cc_apply_ex(CcBoard *board, int from, int to, int promo,
                     CcMoveInfo *info);
void cc_move_uci(uint32_t packed, char out[CC_UCI_BUFFER_SIZE]);
CcResult cc_move_uci_n(uint32_t packed, char *out, size_t capacity,
                       size_t *required);
int cc_piece_at(CcBoard *board, int square);
int cc_side(CcBoard *board);
int cc_in_check(CcBoard *board);
int cc_status(CcBoard *board);
int cc_evaluate(CcBoard *board);
void cc_position_info(CcBoard *board, CcPositionInfo *out);
CcResult cc_position_info_ex(CcBoard *board, CcPositionInfo *out);
void cc_search(CcBoard *board, int max_depth, int movetime_ms, CcSearchResult *out);
CcResult cc_search_ex(CcBoard *board, int max_depth, int movetime_ms,
                      CcSearchResult *out);

#ifdef __cplusplus
}
#endif

#endif
