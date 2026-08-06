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
    pub history_bonus_max: i32,
    pub lmr_base: i32,
    pub lmr_div: i32,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            razor_base: 300,
            razor_mult: 200,
            rfp_margin: 80,
            nmp_base: 3,
            nmp_eval_div: 200,
            nmp_max: 3,
            futility_mult: 79,
            futility_base: 127,
            lmp_base: 2818,
            lmp_depth_mult: 1351,
            history_bonus_max: 1200,
            lmr_base: 85,
            lmr_div: 215,
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
        default: 300,
        min: 0,
        max: 600,
    },
    ParamInfo {
        name: "RazorMult",
        default: 200,
        min: 0,
        max: 400,
    },
    ParamInfo {
        name: "RfpMargin",
        default: 80,
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
        default: 200,
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
        default: 79,
        min: 20,
        max: 200,
    },
    ParamInfo {
        name: "FutilityBase",
        default: 127,
        min: 0,
        max: 300,
    },
    ParamInfo {
        name: "LmpBase",
        default: 2818,
        min: 1000,
        max: 5000,
    },
    ParamInfo {
        name: "LmpDepthMult",
        default: 1351,
        min: 500,
        max: 3000,
    },
    ParamInfo {
        name: "HistoryBonusMax",
        default: 1200,
        min: 400,
        max: 2400,
    },
    ParamInfo {
        name: "LmrBase",
        default: 85,
        min: 0,
        max: 250,
    },
    ParamInfo {
        name: "LmrDiv",
        default: 215,
        min: 150,
        max: 320,
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
            "historybonusmax" => self.history_bonus_max = value,
            "lmrbase" => self.lmr_base = value,
            "lmrdiv" => self.lmr_div = value,
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
}
