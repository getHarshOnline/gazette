//! Spend-only accounting used by compound source observations.

use gazette_core::source::{BudgetExceeded, MetadataBudget};

#[test]
fn charges_are_cumulative_and_failed_charges_preserve_the_balance() {
    let mut budget = MetadataBudget::new(10);
    budget.charge(4).expect("first read");
    assert_eq!(budget.remaining(), 6);
    assert_eq!(budget.charge(7), Err(BudgetExceeded));
    assert_eq!(budget.remaining(), 6);
    budget.charge(6).expect("exact remaining allowance");
    assert_eq!(budget.remaining(), 0);
    assert_eq!(budget.charge(1), Err(BudgetExceeded));
    budget.charge(0).expect("empty read");
}

#[test]
fn maximum_budget_does_not_wrap_on_an_excessive_following_charge() {
    let mut budget = MetadataBudget::new(usize::MAX);
    budget.charge(1).expect("first byte");
    assert_eq!(budget.charge(usize::MAX), Err(BudgetExceeded));
    assert_eq!(budget.remaining(), usize::MAX - 1);
}
