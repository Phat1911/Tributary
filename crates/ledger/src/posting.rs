//! Posting-rule templates: the ONLY way ledger entries are created.
//!
//! There is no `insert_entry` anywhere in the codebase — callers name a
//! TEMPLATE plus amounts and owner references, and the template expands to
//! fixed legs. Free-form postings (caller-chosen accounts, directions,
//! amounts) do not exist as an API, so an unbalanced or nonsensical
//! transaction is a compile-time or validation error, never a DB state.
//!
//! Every template encodes one §11 economic event:
//!   prepaid.fund        renter prepays cash            → escrow ↑, prepaid ↑
//!   prepaid.consume     metered usage settles          → prepaid ↓, revenue ↑
//!   settle.fee          operator takes its fee         → revenue ↓, equity ↑
//!   settle.distribute   distributable to a depositor   → revenue ↓, payable ↑
//!   deposit.capacity    depositor contributes keys     → capacity ↑, claims ↑
//!   withdraw.payable    depositor cashes out earnings  → payable ↓, escrow ↓

use crate::chart::{AccountCode, Direction, OwnerKind, PLATFORM_OWNER_ID};
use crate::error::LedgerError;
use crate::money::MicroUsd;
use std::str::FromStr;

/// Identifies a posting-rule template (stored on every entry as
/// `ledger_entries.template_code` — the audit trail of which rule fired).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateCode {
    PrepaidFund,
    PrepaidConsume,
    SettleFee,
    SettleDistribute,
    DepositCapacity,
    WithdrawPayable,
}

impl TemplateCode {
    /// String form used in the DB column and request validation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrepaidFund => "prepaid.fund",
            Self::PrepaidConsume => "prepaid.consume",
            Self::SettleFee => "settle.fee",
            Self::SettleDistribute => "settle.distribute",
            Self::DepositCapacity => "deposit.capacity",
            Self::WithdrawPayable => "withdraw.payable",
        }
    }

    /// The template definition (legs + description) — see [`Template`].
    pub fn definition(self) -> &'static Template {
        // Backed by the seed-completeness test in this module: every enum
        // variant has exactly one row in TEMPLATES.
        TEMPLATES
            .iter()
            .find(|t| t.code == self)
            .expect("template definition")
    }
}

impl FromStr for TemplateCode {
    type Err = LedgerError;

    /// Parses a template code string (loud failure on unknown codes —
    /// never a fallback template).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "prepaid.fund" => Ok(Self::PrepaidFund),
            "prepaid.consume" => Ok(Self::PrepaidConsume),
            "settle.fee" => Ok(Self::SettleFee),
            "settle.distribute" => Ok(Self::SettleDistribute),
            "deposit.capacity" => Ok(Self::DepositCapacity),
            "withdraw.payable" => Ok(Self::WithdrawPayable),
            other => Err(LedgerError::UnknownTemplate(other.to_string())),
        }
    }
}

/// Which owner an expansion slot binds to. `Platform` resolves to the
/// operator-level account (owner `platform`); `Renter`/`Depositor` take the
/// owner id from the posting request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartySlot {
    Platform,
    Renter,
    Depositor,
}

/// One leg of a template: a FIXED account code, direction, and owner slot.
/// Amounts are not part of the spec — the request carries one amount (all
/// M7 templates are two-legged with equal legs; multi-amount templates
/// arrive when real settlement splits, still as typed fields, never raw).
#[derive(Debug, Clone, Copy)]
pub struct LegSpec {
    pub account: AccountCode,
    pub direction: Direction,
    pub party: PartySlot,
}

/// A posting-rule template: code, human meaning, fixed legs.
#[derive(Debug)]
pub struct Template {
    pub code: TemplateCode,
    pub description: &'static str,
    pub legs: &'static [LegSpec],
}

/// The normative template set. Balanced BY CONSTRUCTION (one debit + one
/// credit leg each) — the property tests then verify the DB agrees.
pub const TEMPLATES: &[Template] = &[
    Template {
        code: TemplateCode::PrepaidFund,
        description: "Renter prepays custodial cash (Dr cash.escrow, Cr prepaid.renter). \
                      Rule 7 loosens: cash rises with the debt it backs.",
        legs: &[
            LegSpec {
                account: AccountCode::CashEscrow,
                direction: Direction::Debit,
                party: PartySlot::Platform,
            },
            LegSpec {
                account: AccountCode::PrepaidRenter,
                direction: Direction::Credit,
                party: PartySlot::Renter,
            },
        ],
    },
    Template {
        code: TemplateCode::PrepaidConsume,
        description: "Metered usage settles against a renter's prepaid (Dr prepaid.renter, \
                      Cr revenue.usage). Sufficiency: prepaid must cover the amount.",
        legs: &[
            LegSpec {
                account: AccountCode::PrepaidRenter,
                direction: Direction::Debit,
                party: PartySlot::Renter,
            },
            LegSpec {
                account: AccountCode::RevenueUsage,
                direction: Direction::Credit,
                party: PartySlot::Platform,
            },
        ],
    },
    Template {
        code: TemplateCode::SettleFee,
        description: "Operator takes its fee from settled revenue (Dr revenue.usage, \
                      Cr equity.operator). Sufficiency: revenue must cover the amount.",
        legs: &[
            LegSpec {
                account: AccountCode::RevenueUsage,
                direction: Direction::Debit,
                party: PartySlot::Platform,
            },
            LegSpec {
                account: AccountCode::EquityOperator,
                direction: Direction::Credit,
                party: PartySlot::Platform,
            },
        ],
    },
    Template {
        code: TemplateCode::SettleDistribute,
        description: "Distributable revenue is owed to a depositor (Dr revenue.usage, \
                      Cr payable.depositor). Sufficiency: revenue must cover the amount.",
        legs: &[
            LegSpec {
                account: AccountCode::RevenueUsage,
                direction: Direction::Debit,
                party: PartySlot::Platform,
            },
            LegSpec {
                account: AccountCode::PayableDepositor,
                direction: Direction::Credit,
                party: PartySlot::Depositor,
            },
        ],
    },
    Template {
        code: TemplateCode::DepositCapacity,
        description: "Depositor contributes key capacity (Dr capacity.contributed, \
                      Cr claims.contributed). Outside rule 7: no cash moved (D38).",
        legs: &[
            LegSpec {
                account: AccountCode::CapacityContributed,
                direction: Direction::Debit,
                party: PartySlot::Depositor,
            },
            LegSpec {
                account: AccountCode::ClaimsContributed,
                direction: Direction::Credit,
                party: PartySlot::Depositor,
            },
        ],
    },
    Template {
        code: TemplateCode::WithdrawPayable,
        description: "Depositor cashes out settled earnings (Dr payable.depositor, \
                      Cr cash.escrow). Sufficiency: payable must cover the amount.",
        legs: &[
            LegSpec {
                account: AccountCode::PayableDepositor,
                direction: Direction::Debit,
                party: PartySlot::Depositor,
            },
            LegSpec {
                account: AccountCode::CashEscrow,
                direction: Direction::Credit,
                party: PartySlot::Platform,
            },
        ],
    },
];

/// What a caller asks the ledger to post: a template by name, one amount,
/// the owner references the template's slots bind to, and an optional
/// external reference. There is deliberately NO field for accounts,
/// directions, or a leg list — callers cannot free-form.
#[derive(Debug, Clone, Copy)]
pub struct PostingRequest<'a> {
    pub template: TemplateCode,
    pub amount: MicroUsd,
    pub renter: Option<&'a str>,
    pub depositor: Option<&'a str>,
    pub reference: &'a str,
}

impl<'a> PostingRequest<'a> {
    /// Convenience constructor for the common shape (template + amount).
    pub fn new(template: TemplateCode, amount: MicroUsd) -> Self {
        Self {
            template,
            amount,
            renter: None,
            depositor: None,
            reference: "",
        }
    }

    /// Binds the renter slot (templates with a `Renter` leg require this).
    pub fn renter(mut self, renter: &'a str) -> Self {
        self.renter = Some(renter);
        self
    }

    /// Binds the depositor slot (templates with a `Depositor` leg require
    /// this).
    pub fn depositor(mut self, depositor: &'a str) -> Self {
        self.depositor = Some(depositor);
        self
    }

    /// Attaches an external reference (checkout id, usage-event id, …).
    pub fn reference(mut self, reference: &'a str) -> Self {
        self.reference = reference;
        self
    }
}

/// A template leg with its owner slot RESOLVED to a concrete owner — the
/// intermediate form `SqlLedger` turns into entry rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedLeg {
    pub account: AccountCode,
    pub direction: Direction,
    pub owner_kind: OwnerKind,
    pub owner_id: String,
}

/// Expands a request against its template: validates the amount and that
/// exactly the owner slots the template uses are bound (a missing slot is
/// a caller bug; a superfluous one is a caller bug too — strictness here
/// catches template/request drift at the boundary).
pub fn expand(request: &PostingRequest<'_>) -> Result<Vec<ExpandedLeg>, LedgerError> {
    if request.amount.micros() <= 0 {
        return Err(LedgerError::OwnerSlot {
            template: request.template.as_str().to_string(),
            problem: format!("amount must be positive, got {}", request.amount.micros()),
        });
    }
    let template = request.template.definition();
    let mut legs = Vec::with_capacity(template.legs.len());
    for spec in template.legs {
        let (kind, id) = match spec.party {
            PartySlot::Platform => (OwnerKind::Operator, PLATFORM_OWNER_ID.to_string()),
            PartySlot::Renter => match request.renter {
                Some(id) => (OwnerKind::Renter, id.to_string()),
                None => {
                    return Err(LedgerError::OwnerSlot {
                        template: template.code.as_str().to_string(),
                        problem: "template requires a renter; none bound".to_string(),
                    });
                }
            },
            PartySlot::Depositor => match request.depositor {
                Some(id) => (OwnerKind::Depositor, id.to_string()),
                None => {
                    return Err(LedgerError::OwnerSlot {
                        template: template.code.as_str().to_string(),
                        problem: "template requires a depositor; none bound".to_string(),
                    });
                }
            },
        };
        legs.push(ExpandedLeg {
            account: spec.account,
            direction: spec.direction,
            owner_kind: kind,
            owner_id: id,
        });
    }
    // Strictness: slots bound but unused mean the caller thinks a different
    // template is firing — refuse rather than silently ignore.
    let uses_renter = template.legs.iter().any(|l| l.party == PartySlot::Renter);
    let uses_depositor = template
        .legs
        .iter()
        .any(|l| l.party == PartySlot::Depositor);
    if !uses_renter && request.renter.is_some() {
        return Err(LedgerError::OwnerSlot {
            template: template.code.as_str().to_string(),
            problem: "request binds a renter but the template has no renter leg".to_string(),
        });
    }
    if !uses_depositor && request.depositor.is_some() {
        return Err(LedgerError::OwnerSlot {
            template: template.code.as_str().to_string(),
            problem: "request binds a depositor but the template has no depositor leg".to_string(),
        });
    }
    Ok(legs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_code_has_exactly_one_definition() {
        let all = [
            TemplateCode::PrepaidFund,
            TemplateCode::PrepaidConsume,
            TemplateCode::SettleFee,
            TemplateCode::SettleDistribute,
            TemplateCode::DepositCapacity,
            TemplateCode::WithdrawPayable,
        ];
        assert_eq!(TEMPLATES.len(), all.len());
        for code in all {
            assert_eq!(code.definition().code, code);
        }
    }

    #[test]
    fn every_template_balances_by_construction() {
        for template in TEMPLATES {
            let debits = template
                .legs
                .iter()
                .filter(|l| l.direction == Direction::Debit)
                .count();
            let credits = template
                .legs
                .iter()
                .filter(|l| l.direction == Direction::Credit)
                .count();
            assert!(
                debits >= 1 && credits >= 1,
                "{}: template must have debit AND credit legs",
                template.code.as_str()
            );
        }
    }

    #[test]
    fn expansion_binds_owner_slots_strictly() {
        // Missing renter → loud.
        let req = PostingRequest::new(TemplateCode::PrepaidFund, MicroUsd::new(500));
        assert!(matches!(expand(&req), Err(LedgerError::OwnerSlot { .. })));
        // Bound and used → two resolved legs, one platform, one renter.
        let legs = expand(&req.renter("renter-1")).unwrap();
        assert_eq!(legs.len(), 2);
        assert!(legs.iter().any(|l| l.owner_id == "platform"));
        assert!(
            legs.iter()
                .any(|l| l.owner_id == "renter-1" && l.owner_kind == OwnerKind::Renter)
        );
        // Bound but unused → loud (caller/template drift).
        let stray = PostingRequest::new(TemplateCode::SettleFee, MicroUsd::new(1)).renter("r");
        assert!(expand(&stray).is_err());
        // Non-positive amount → loud.
        let zero = PostingRequest::new(TemplateCode::SettleFee, MicroUsd::new(0));
        assert!(expand(&zero).is_err());
    }
}
