use cosmwasm_std::testing::{MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    from_slice, to_binary, Api, CanonicalAddr, Coin, Decimal, Extern, HumanAddr, Querier,
    QuerierResult, QueryRequest, StdError, SystemError, Uint128, WasmQuery,
};
use cosmwasm_storage::to_length_prefixed;

use std::collections::HashMap;

use crate::querier::WhitelistInfo;
use terra_cosmwasm::{TaxCapResponse, TaxRateResponse, TerraQuery, TerraQueryWrapper};

/// mock_dependencies is a drop-in replacement for cosmwasm_std::testing::mock_dependencies
/// this uses our CustomQuerier.
pub fn mock_dependencies(
    canonical_length: usize,
    contract_balance: &[Coin],
) -> Extern<MockStorage, MockApi, WasmMockQuerier> {
    let contract_addr = HumanAddr::from(MOCK_CONTRACT_ADDR);
    let custom_querier: WasmMockQuerier = WasmMockQuerier::new(
        MockQuerier::new(&[(&contract_addr, contract_balance)]),
        MockApi::new(canonical_length),
        canonical_length,
    );

    Extern {
        storage: MockStorage::default(),
        api: MockApi::new(canonical_length),
        querier: custom_querier,
    }
}

pub struct WasmMockQuerier {
    base: MockQuerier<TerraQueryWrapper>,
    token_querier: TokenQuerier,
    tax_querier: TaxQuerier,
    whitelist_querier: WhitelistQuerier,
    canonical_length: usize,
}

#[derive(Clone, Default)]
pub struct TokenQuerier {
    // this lets us iterate over all pairs that match the first string
    balances: HashMap<HumanAddr, HashMap<HumanAddr, Uint128>>,
}

impl TokenQuerier {
    pub fn new(balances: &[(&HumanAddr, &[(&HumanAddr, &Uint128)])]) -> Self {
        TokenQuerier {
            balances: balances_to_map(balances),
        }
    }
}

pub(crate) fn balances_to_map(
    balances: &[(&HumanAddr, &[(&HumanAddr, &Uint128)])],
) -> HashMap<HumanAddr, HashMap<HumanAddr, Uint128>> {
    let mut balances_map: HashMap<HumanAddr, HashMap<HumanAddr, Uint128>> = HashMap::new();
    for (contract_addr, balances) in balances.iter() {
        let mut contract_balances_map: HashMap<HumanAddr, Uint128> = HashMap::new();
        for (addr, balance) in balances.iter() {
            contract_balances_map.insert(HumanAddr::from(addr), **balance);
        }

        balances_map.insert(HumanAddr::from(contract_addr), contract_balances_map);
    }
    balances_map
}

#[derive(Clone, Default)]
pub struct TaxQuerier {
    rate: Decimal,
    // this lets us iterate over all pairs that match the first string
    caps: HashMap<String, Uint128>,
}

impl TaxQuerier {
    pub fn new(rate: Decimal, caps: &[(&String, &Uint128)]) -> Self {
        TaxQuerier {
            rate,
            caps: caps_to_map(caps),
        }
    }
}

pub(crate) fn caps_to_map(caps: &[(&String, &Uint128)]) -> HashMap<String, Uint128> {
    let mut owner_map: HashMap<String, Uint128> = HashMap::new();
    for (denom, cap) in caps.iter() {
        owner_map.insert(denom.to_string(), **cap);
    }
    owner_map
}

#[derive(Clone, Debug)]
pub struct WhitelistItem {
    pub token_contract: HumanAddr,
    pub uniswap_contract: HumanAddr,
}

#[derive(Clone, Default)]
pub struct WhitelistQuerier {
    // this lets us iterate over all pairs that match the first string
    whitelist: HashMap<HumanAddr, HashMap<HumanAddr, WhitelistItem>>,
}

impl WhitelistQuerier {
    pub fn new(whitelist: &[(&HumanAddr, Vec<(&HumanAddr, &WhitelistItem)>)]) -> Self {
        WhitelistQuerier {
            whitelist: whitelist_to_map(whitelist),
        }
    }
}

pub(crate) fn whitelist_to_map(
    whitelists: &[(&HumanAddr, Vec<(&HumanAddr, &WhitelistItem)>)],
) -> HashMap<HumanAddr, HashMap<HumanAddr, WhitelistItem>> {
    let mut whitelists_map: HashMap<HumanAddr, HashMap<HumanAddr, WhitelistItem>> = HashMap::new();
    for (contract, whitelist) in whitelists.iter() {
        let mut whitelist_map: HashMap<HumanAddr, WhitelistItem> = HashMap::new();
        for (symbol, item) in whitelist.iter() {
            whitelist_map.insert((*symbol).clone(), (*item).clone());
        }

        whitelists_map.insert(HumanAddr::from(contract), whitelist_map);
    }

    whitelists_map
}

impl Querier for WasmMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        // MockQuerier doesn't support Custom, so we ignore it completely here
        let request: QueryRequest<TerraQueryWrapper> = match from_slice(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return Err(SystemError::InvalidRequest {
                    error: format!("Parsing query request: {}", e),
                    request: bin_request.into(),
                })
            }
        };
        self.handle_query(&request)
    }
}

impl WasmMockQuerier {
    pub fn handle_query(&self, request: &QueryRequest<TerraQueryWrapper>) -> QuerierResult {
        match &request {
            QueryRequest::Custom(TerraQueryWrapper { route, query_data }) => {
                let route_str = route.as_str();
                if route_str == "treasury" {
                    match query_data {
                        TerraQuery::TaxRate {} => {
                            let res = TaxRateResponse {
                                rate: self.tax_querier.rate,
                            };
                            Ok(to_binary(&res))
                        }
                        TerraQuery::TaxCap { denom } => {
                            let cap = self
                                .tax_querier
                                .caps
                                .get(denom)
                                .copied()
                                .unwrap_or_default();
                            let res = TaxCapResponse { cap };
                            Ok(to_binary(&res))
                        }
                        _ => panic!("DO NOT ENTER HERE"),
                    }
                } else {
                    panic!("DO NOT ENTER HERE")
                }
            }
            QueryRequest::Wasm(WasmQuery::Raw { contract_addr, key }) => {
                let key: &[u8] = key.as_slice();
                let prefix_balance = to_length_prefixed(b"balance").to_vec();
                let prefix_whitelist = to_length_prefixed(b"whitelist").to_vec();

                if key.len() > prefix_whitelist.len()
                    && key[..prefix_whitelist.len()].to_vec() == prefix_whitelist
                {
                    let api: MockApi = MockApi::new(self.canonical_length);
                    let key_address: &[u8] = &key[prefix_whitelist.len()..];
                    let addr = api
                        .human_address(&CanonicalAddr::from(key_address))
                        .unwrap();

                    let item = match self.whitelist_querier.whitelist.get(&contract_addr) {
                        Some(whitelist) => match whitelist.get(&addr) {
                            Some(v) => v,
                            None => {
                                return Ok(Err(StdError::generic_err(format!(
                                    "No whitelist info registered for {} {}",
                                    contract_addr, addr,
                                ))))
                            }
                        },
                        None => {
                            return Ok(Err(StdError::generic_err(format!(
                                "No whitelist info registered for {}",
                                contract_addr,
                            ))))
                        }
                    };

                    Ok(to_binary(
                        &to_binary(&WhitelistInfo {
                            uniswap_contract: api
                                .canonical_address(&item.uniswap_contract)
                                .unwrap(),
                            token_contract: api.canonical_address(&item.token_contract).unwrap(),
                        })
                        .unwrap(),
                    ))
                } else {
                    let balances: &HashMap<HumanAddr, Uint128> =
                        match self.token_querier.balances.get(contract_addr) {
                            Some(balances) => balances,
                            None => {
                                return Err(SystemError::InvalidRequest {
                                    error: format!(
                                        "No balance info exists for the contract {}",
                                        contract_addr
                                    ),
                                    request: key.into(),
                                })
                            }
                        };

                    if key[..prefix_balance.len()].to_vec() == prefix_balance {
                        let key_address: &[u8] = &key[prefix_balance.len()..];
                        let address_raw: CanonicalAddr = CanonicalAddr::from(key_address);

                        let api: MockApi = MockApi::new(self.canonical_length);
                        let address: HumanAddr = match api.human_address(&address_raw) {
                            Ok(v) => v,
                            Err(e) => {
                                return Err(SystemError::InvalidRequest {
                                    error: format!("Parsing query request: {}", e),
                                    request: key.into(),
                                })
                            }
                        };

                        let balance = match balances.get(&address) {
                            Some(v) => v,
                            None => {
                                return Err(SystemError::InvalidRequest {
                                    error: "Balance not found".to_string(),
                                    request: key.into(),
                                })
                            }
                        };

                        Ok(to_binary(&to_binary(&balance).unwrap()))
                    } else {
                        panic!("DO NOT ENTER HERE")
                    }
                }
            }
            _ => self.base.handle_query(request),
        }
    }
}

impl WasmMockQuerier {
    pub fn new<A: Api>(
        base: MockQuerier<TerraQueryWrapper>,
        _api: A,
        canonical_length: usize,
    ) -> Self {
        WasmMockQuerier {
            base,
            token_querier: TokenQuerier::default(),
            tax_querier: TaxQuerier::default(),
            whitelist_querier: WhitelistQuerier::default(),
            canonical_length,
        }
    }

    // configure the mint whitelist mock querier
    pub fn with_token_balances(&mut self, balances: &[(&HumanAddr, &[(&HumanAddr, &Uint128)])]) {
        self.token_querier = TokenQuerier::new(balances);
    }

    // configure the token owner mock querier
    pub fn with_tax(&mut self, rate: Decimal, caps: &[(&String, &Uint128)]) {
        self.tax_querier = TaxQuerier::new(rate, caps);
    }

    // configure the whitelist mock querier
    pub fn with_whitelist(
        &mut self,
        whitelists: &[(&HumanAddr, Vec<(&HumanAddr, &WhitelistItem)>)],
    ) {
        self.whitelist_querier = WhitelistQuerier::new(whitelists);
    }
}
