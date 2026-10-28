use std::fmt;

/// A resource type number as stored in KEY, BIF, ERF and RIM tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResType(pub u32);

impl ResType {
    pub const RES: ResType = ResType(0);
    pub const BMP: ResType = ResType(1);
    pub const MVE: ResType = ResType(2);
    pub const TGA: ResType = ResType(3);
    pub const WAV: ResType = ResType(4);
    pub const PLT: ResType = ResType(6);
    pub const INI: ResType = ResType(7);
    pub const BMU: ResType = ResType(8);
    pub const MPG: ResType = ResType(9);
    pub const TXT: ResType = ResType(10);
    pub const PLH: ResType = ResType(2000);
    pub const TEX: ResType = ResType(2001);
    pub const MDL: ResType = ResType(2002);
    pub const THG: ResType = ResType(2003);
    pub const FNT: ResType = ResType(2005);
    pub const LUA: ResType = ResType(2007);
    pub const SLT: ResType = ResType(2008);
    pub const NSS: ResType = ResType(2009);
    pub const NCS: ResType = ResType(2010);
    pub const MOD: ResType = ResType(2011);
    pub const ARE: ResType = ResType(2012);
    pub const SET: ResType = ResType(2013);
    pub const IFO: ResType = ResType(2014);
    pub const BIC: ResType = ResType(2015);
    pub const WOK: ResType = ResType(2016);
    pub const TWO_DA: ResType = ResType(2017);
    pub const TLK: ResType = ResType(2018);
    pub const TXI: ResType = ResType(2022);
    pub const GIT: ResType = ResType(2023);
    pub const BTI: ResType = ResType(2024);
    pub const UTI: ResType = ResType(2025);
    pub const BTC: ResType = ResType(2026);
    pub const UTC: ResType = ResType(2027);
    pub const DLG: ResType = ResType(2029);
    pub const ITP: ResType = ResType(2030);
    pub const BTT: ResType = ResType(2031);
    pub const UTT: ResType = ResType(2032);
    pub const DDS: ResType = ResType(2033);
    pub const BTS: ResType = ResType(2034);
    pub const UTS: ResType = ResType(2035);
    pub const LTR: ResType = ResType(2036);
    pub const GFF: ResType = ResType(2037);
    pub const FAC: ResType = ResType(2038);
    pub const BTE: ResType = ResType(2039);
    pub const UTE: ResType = ResType(2040);
    pub const BTD: ResType = ResType(2041);
    pub const UTD: ResType = ResType(2042);
    pub const BTP: ResType = ResType(2043);
    pub const UTP: ResType = ResType(2044);
    pub const DFT: ResType = ResType(2045);
    pub const GIC: ResType = ResType(2046);
    pub const GUI: ResType = ResType(2047);
    pub const CSS: ResType = ResType(2048);
    pub const CCS: ResType = ResType(2049);
    pub const BTM: ResType = ResType(2050);
    pub const UTM: ResType = ResType(2051);
    pub const DWK: ResType = ResType(2052);
    pub const PWK: ResType = ResType(2053);
    pub const BTG: ResType = ResType(2054);
    pub const UTG: ResType = ResType(2055);
    pub const JRL: ResType = ResType(2056);
    pub const SAV: ResType = ResType(2057);
    pub const UTW: ResType = ResType(2058);
    pub const FOUR_PC: ResType = ResType(2059);
    pub const SSF: ResType = ResType(2060);
    pub const HAK: ResType = ResType(2061);
    pub const NWM: ResType = ResType(2062);
    pub const BIK: ResType = ResType(2063);
    pub const NDB: ResType = ResType(2064);
    pub const PTM: ResType = ResType(2065);
    pub const PTT: ResType = ResType(2066);
    pub const LYT: ResType = ResType(3000);
    pub const VIS: ResType = ResType(3001);
    pub const RIM: ResType = ResType(3002);
    pub const PTH: ResType = ResType(3003);
    pub const LIP: ResType = ResType(3004);
    pub const BWM: ResType = ResType(3005);
    pub const TXB: ResType = ResType(3006);
    pub const TPC: ResType = ResType(3007);
    pub const MDX: ResType = ResType(3008);
    pub const RSV: ResType = ResType(3009);
    pub const SIG: ResType = ResType(3010);
    pub const ERF: ResType = ResType(9997);
    pub const BIF: ResType = ResType(9998);
    pub const KEY: ResType = ResType(9999);
}

/// The type numbers the Odyssey games use, with their file extensions.
const TYPES: &[(ResType, &str)] = &[
    (ResType::RES, "res"),
    (ResType::BMP, "bmp"),
    (ResType::MVE, "mve"),
    (ResType::TGA, "tga"),
    (ResType::WAV, "wav"),
    (ResType::PLT, "plt"),
    (ResType::INI, "ini"),
    (ResType::BMU, "bmu"),
    (ResType::MPG, "mpg"),
    (ResType::TXT, "txt"),
    (ResType::PLH, "plh"),
    (ResType::TEX, "tex"),
    (ResType::MDL, "mdl"),
    (ResType::THG, "thg"),
    (ResType::FNT, "fnt"),
    (ResType::LUA, "lua"),
    (ResType::SLT, "slt"),
    (ResType::NSS, "nss"),
    (ResType::NCS, "ncs"),
    (ResType::MOD, "mod"),
    (ResType::ARE, "are"),
    (ResType::SET, "set"),
    (ResType::IFO, "ifo"),
    (ResType::BIC, "bic"),
    (ResType::WOK, "wok"),
    (ResType::TWO_DA, "2da"),
    (ResType::TLK, "tlk"),
    (ResType::TXI, "txi"),
    (ResType::GIT, "git"),
    (ResType::BTI, "bti"),
    (ResType::UTI, "uti"),
    (ResType::BTC, "btc"),
    (ResType::UTC, "utc"),
    (ResType::DLG, "dlg"),
    (ResType::ITP, "itp"),
    (ResType::BTT, "btt"),
    (ResType::UTT, "utt"),
    (ResType::DDS, "dds"),
    (ResType::BTS, "bts"),
    (ResType::UTS, "uts"),
    (ResType::LTR, "ltr"),
    (ResType::GFF, "gff"),
    (ResType::FAC, "fac"),
    (ResType::BTE, "bte"),
    (ResType::UTE, "ute"),
    (ResType::BTD, "btd"),
    (ResType::UTD, "utd"),
    (ResType::BTP, "btp"),
    (ResType::UTP, "utp"),
    (ResType::DFT, "dft"),
    (ResType::GIC, "gic"),
    (ResType::GUI, "gui"),
    (ResType::CSS, "css"),
    (ResType::CCS, "ccs"),
    (ResType::BTM, "btm"),
    (ResType::UTM, "utm"),
    (ResType::DWK, "dwk"),
    (ResType::PWK, "pwk"),
    (ResType::BTG, "btg"),
    (ResType::UTG, "utg"),
    (ResType::JRL, "jrl"),
    (ResType::SAV, "sav"),
    (ResType::UTW, "utw"),
    (ResType::FOUR_PC, "4pc"),
    (ResType::SSF, "ssf"),
    (ResType::HAK, "hak"),
    (ResType::NWM, "nwm"),
    (ResType::BIK, "bik"),
    (ResType::NDB, "ndb"),
    (ResType::PTM, "ptm"),
    (ResType::PTT, "ptt"),
    (ResType::LYT, "lyt"),
    (ResType::VIS, "vis"),
    (ResType::RIM, "rim"),
    (ResType::PTH, "pth"),
    (ResType::LIP, "lip"),
    (ResType::BWM, "bwm"),
    (ResType::TXB, "txb"),
    (ResType::TPC, "tpc"),
    (ResType::MDX, "mdx"),
    (ResType::RSV, "rsv"),
    (ResType::SIG, "sig"),
    (ResType::ERF, "erf"),
    (ResType::BIF, "bif"),
    (ResType::KEY, "key"),
];

impl ResType {
    /// The file extension, or `None` for a number outside the table.
    pub fn extension(self) -> Option<&'static str> {
        TYPES
            .iter()
            .find(|(known, _)| *known == self)
            .map(|(_, extension)| *extension)
    }

    pub fn from_extension(extension: &str) -> Option<ResType> {
        TYPES
            .iter()
            .find(|(_, known)| known.eq_ignore_ascii_case(extension))
            .map(|(known, _)| *known)
    }
}

impl fmt::Display for ResType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.extension() {
            Some(extension) => f.write_str(extension),
            None => write!(f, "unknown({})", self.0),
        }
    }
}
