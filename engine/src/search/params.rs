#[derive(Clone, Copy)]
pub struct Params {
    pub razor_base: i32,
    pub razor_mult: i32,
    pub rfp_margin: i32,
    pub nmp_base: i32,
    pub nmp_eval_div: i32,
    pub nmp_max: i32,
    pub futility_mult: i32,
    pub futility_base: i32,
    pub lmp_base: i32,
    pub lmp_depth_mult: i32,
    pub lmp_improvement_mult: i32,
    pub pruning_history_div: i32,
    pub futility_eval_bonus: i32,
    pub futility_max_depth: i32,
    pub history_pruning_max_depth: i32,
    pub history_pruning_mult: i32,
    pub see_quiet_depth_mult: i32,
    pub see_quiet_linear: i32,
    pub see_quiet_base: i32,
    pub see_noisy_depth_mult: i32,
    pub see_noisy_linear: i32,
    pub see_history_div: i32,
    pub qsearch_see_margin: i32,
    pub history_bonus_max: i32,
    pub lmr_base: i32,
    pub lmr_div: i32,
    pub lmr_noisy_base: i32,
    pub lmr_noisy_div: i32,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            razor_base: 349,
            razor_mult: 191,
            rfp_margin: 95,
            nmp_base: 3,
            nmp_eval_div: 183,
            nmp_max: 3,
            futility_mult: 87,
            futility_base: 85,
            lmp_base: 3072,
            lmp_depth_mult: 1024,
            lmp_improvement_mult: 4,
            pruning_history_div: 16,
            futility_eval_bonus: 64,
            futility_max_depth: 12,
            history_pruning_max_depth: 4,
            history_pruning_mult: 1024,
            see_quiet_depth_mult: 15,
            see_quiet_linear: 45,
            see_quiet_base: 20,
            see_noisy_depth_mult: 10,
            see_noisy_linear: 30,
            see_history_div: 32,
            qsearch_see_margin: 64,
            history_bonus_max: 1288,
            lmr_base: 82,
            lmr_div: 217,
            lmr_noisy_base: 40,
            lmr_noisy_div: 300,
        }
    }
}

pub struct ParamInfo {
    pub name: &'static str,
    pub default: i32,
    pub min: i32,
    pub max: i32,
}

pub const UCI_OPTIONS: &[ParamInfo] = &[
    ParamInfo {
        name: "RazorBase",
        default: 349,
        min: 0,
        max: 600,
    },
    ParamInfo {
        name: "RazorMult",
        default: 191,
        min: 0,
        max: 400,
    },
    ParamInfo {
        name: "RfpMargin",
        default: 95,
        min: 20,
        max: 200,
    },
    ParamInfo {
        name: "NmpBase",
        default: 3,
        min: 1,
        max: 6,
    },
    ParamInfo {
        name: "NmpEvalDiv",
        default: 183,
        min: 50,
        max: 400,
    },
    ParamInfo {
        name: "NmpMax",
        default: 3,
        min: 1,
        max: 6,
    },
    ParamInfo {
        name: "FutilityMult",
        default: 87,
        min: 20,
        max: 200,
    },
    ParamInfo {
        name: "FutilityBase",
        default: 85,
        min: 0,
        max: 300,
    },
    ParamInfo {
        name: "LmpBase",
        default: 3072,
        min: 1000,
        max: 5000,
    },
    ParamInfo {
        name: "LmpDepthMult",
        default: 1024,
        min: 500,
        max: 3000,
    },
    ParamInfo {
        name: "LmpImprovementMult",
        default: 4,
        min: 0,
        max: 16,
    },
    ParamInfo {
        name: "PruningHistoryDiv",
        default: 16,
        min: 1,
        max: 128,
    },
    ParamInfo {
        name: "FutilityEvalBonus",
        default: 64,
        min: 0,
        max: 256,
    },
    ParamInfo {
        name: "FutilityMaxDepth",
        default: 12,
        min: 1,
        max: 16,
    },
    ParamInfo {
        name: "HistoryPruningMaxDepth",
        default: 4,
        min: 1,
        max: 8,
    },
    ParamInfo {
        name: "HistoryPruningMult",
        default: 1024,
        min: 128,
        max: 4096,
    },
    ParamInfo {
        name: "SeeQuietDepthMult",
        default: 15,
        min: 0,
        max: 64,
    },
    ParamInfo {
        name: "SeeQuietLinear",
        default: 45,
        min: 0,
        max: 128,
    },
    ParamInfo {
        name: "SeeQuietBase",
        default: 20,
        min: 0,
        max: 128,
    },
    ParamInfo {
        name: "SeeNoisyDepthMult",
        default: 10,
        min: 0,
        max: 64,
    },
    ParamInfo {
        name: "SeeNoisyLinear",
        default: 30,
        min: 0,
        max: 128,
    },
    ParamInfo {
        name: "SeeHistoryDiv",
        default: 32,
        min: 1,
        max: 256,
    },
    ParamInfo {
        name: "QsearchSeeMargin",
        default: 64,
        min: 0,
        max: 256,
    },
    ParamInfo {
        name: "HistoryBonusMax",
        default: 1288,
        min: 400,
        max: 2400,
    },
    ParamInfo {
        name: "LmrBase",
        default: 82,
        min: 0,
        max: 250,
    },
    ParamInfo {
        name: "LmrDiv",
        default: 217,
        min: 150,
        max: 320,
    },
    ParamInfo {
        name: "LmrNoisyBase",
        default: 40,
        min: 0,
        max: 250,
    },
    ParamInfo {
        name: "LmrNoisyDiv",
        default: 300,
        min: 100,
        max: 600,
    },
];

impl Params {
    pub fn set(&mut self, name: &str, value: i32) -> bool {
        let name = name.to_ascii_lowercase();
        let Some(info) = UCI_OPTIONS
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(&name))
        else {
            return false;
        };
        let value = value.clamp(info.min, info.max);
        match name.as_str() {
            "razorbase" => self.razor_base = value,
            "razormult" => self.razor_mult = value,
            "rfpmargin" => self.rfp_margin = value,
            "nmpbase" => self.nmp_base = value,
            "nmpevaldiv" => self.nmp_eval_div = value,
            "nmpmax" => self.nmp_max = value,
            "futilitymult" => self.futility_mult = value,
            "futilitybase" => self.futility_base = value,
            "lmpbase" => self.lmp_base = value,
            "lmpdepthmult" => self.lmp_depth_mult = value,
            "lmpimprovementmult" => self.lmp_improvement_mult = value,
            "pruninghistorydiv" => self.pruning_history_div = value,
            "futilityevalbonus" => self.futility_eval_bonus = value,
            "futilitymaxdepth" => self.futility_max_depth = value,
            "historypruningmaxdepth" => self.history_pruning_max_depth = value,
            "historypruningmult" => self.history_pruning_mult = value,
            "seequietdepthmult" => self.see_quiet_depth_mult = value,
            "seequietlinear" => self.see_quiet_linear = value,
            "seequietbase" => self.see_quiet_base = value,
            "seenoisydepthmult" => self.see_noisy_depth_mult = value,
            "seenoisylinear" => self.see_noisy_linear = value,
            "seehistorydiv" => self.see_history_div = value,
            "qsearchseemargin" => self.qsearch_see_margin = value,
            "historybonusmax" => self.history_bonus_max = value,
            "lmrbase" => self.lmr_base = value,
            "lmrdiv" => self.lmr_div = value,
            "lmrnoisybase" => self.lmr_noisy_base = value,
            "lmrnoisydiv" => self.lmr_noisy_div = value,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_values_are_clamped_not_stored_raw() {
        let mut p = Params::default();
        assert!(p.set("NmpEvalDiv", 0));
        assert_eq!(p.nmp_eval_div, 50);
        assert!(p.set("NmpEvalDiv", 100_000));
        assert_eq!(p.nmp_eval_div, 400);
    }

    #[test]
    fn lookup_and_apply_are_both_case_insensitive() {
        let mut p = Params::default();
        assert!(p.set("razorbase", 123));
        assert_eq!(p.razor_base, 123);
    }

    #[test]
    fn unknown_name_is_rejected() {
        let mut p = Params::default();
        assert!(!p.set("NotAThing", 1));
    }

    #[test]
    fn advertised_defaults_match_search_defaults() {
        let p = Params::default();
        let values = [
            ("RazorBase", p.razor_base),
            ("RazorMult", p.razor_mult),
            ("RfpMargin", p.rfp_margin),
            ("NmpBase", p.nmp_base),
            ("NmpEvalDiv", p.nmp_eval_div),
            ("NmpMax", p.nmp_max),
            ("FutilityMult", p.futility_mult),
            ("FutilityBase", p.futility_base),
            ("LmpBase", p.lmp_base),
            ("LmpDepthMult", p.lmp_depth_mult),
            ("LmpImprovementMult", p.lmp_improvement_mult),
            ("PruningHistoryDiv", p.pruning_history_div),
            ("FutilityEvalBonus", p.futility_eval_bonus),
            ("FutilityMaxDepth", p.futility_max_depth),
            ("HistoryPruningMaxDepth", p.history_pruning_max_depth),
            ("HistoryPruningMult", p.history_pruning_mult),
            ("SeeQuietDepthMult", p.see_quiet_depth_mult),
            ("SeeQuietLinear", p.see_quiet_linear),
            ("SeeQuietBase", p.see_quiet_base),
            ("SeeNoisyDepthMult", p.see_noisy_depth_mult),
            ("SeeNoisyLinear", p.see_noisy_linear),
            ("SeeHistoryDiv", p.see_history_div),
            ("QsearchSeeMargin", p.qsearch_see_margin),
            ("HistoryBonusMax", p.history_bonus_max),
            ("LmrBase", p.lmr_base),
            ("LmrDiv", p.lmr_div),
            ("LmrNoisyBase", p.lmr_noisy_base),
            ("LmrNoisyDiv", p.lmr_noisy_div),
        ];
        assert_eq!(values.len(), UCI_OPTIONS.len());
        for (name, value) in values {
            let advertised = UCI_OPTIONS.iter().find(|item| item.name == name).unwrap();
            assert_eq!(value, advertised.default, "{name}");
        }
    }
}
