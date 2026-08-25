use super::*;

#[test]
fn test_center_truncate_string() {
    // Test string shorter than limit - should not be truncated
    assert_eq!(VariableList::center_truncate_string("short", 10), "short");

    // Test exact length - should not be truncated
    assert_eq!(
        VariableList::center_truncate_string("exactly_10", 10),
        "exactly_10"
    );

    // Test simple truncation
    assert_eq!(
        VariableList::center_truncate_string("value->value2->value3->value4", 20),
        "value->v...3->value4"
    );

    // Test with very long expression
    assert_eq!(
        VariableList::center_truncate_string(
            "object->property1->property2->property3->property4->property5",
            30
        ),
        "object->prope...ty4->property5"
    );

    // Test edge case with limit equal to ellipsis length
    assert_eq!(VariableList::center_truncate_string("anything", 3), "any");

    // Test edge case with limit less than ellipsis length
    assert_eq!(VariableList::center_truncate_string("anything", 2), "any");

    // Test with UTF-8 characters
    assert_eq!(
        VariableList::center_truncate_string("café->résumé->naïve->voilà", 15),
        "café->...>voilà"
    );

    // Test with emoji (multi-byte UTF-8)
    assert_eq!(
        VariableList::center_truncate_string("😀->happy->face->😎->cool", 15),
        "😀->hap...->cool"
    );
}
