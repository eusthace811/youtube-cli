import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import report  # noqa: E402


def test_clean_results_end_with_all_registered():
    text, unexplained = report.build([({"id": "a"}, []), ({"id": "b", "divergences": ["E1"]}, [])], [], [])
    assert unexplained == []
    assert text.splitlines()[-1] == "cases: 3, differences: 0, divergences: 15, all registered"


def test_an_injected_unregistered_difference_fails_the_report():
    text, unexplained = report.build([({"id": "a"}, ["stdout: at byte 0: 'x' != 'y'"])], [], [])
    assert unexplained == [("a", "stdout: at byte 0: 'x' != 'y'")]
    assert text.splitlines()[-1] == "cases: 2, differences: 1, divergences: 15, NOT all registered"
    assert "## Unexplained differences" in text


def test_unregistered_contract_differences_fail_the_report():
    _, unexplained = report.build([], ["$.tools: length 74 != 73"], [])
    assert unexplained == [("contract", "$.tools: length 74 != 73")]
