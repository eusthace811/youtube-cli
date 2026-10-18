import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import diff  # noqa: E402

CASE = {"id": "t"}


def result(**over):
    base = {"requests": [{"method": "GET", "path": "/youtube/v3/videos",
                          "query": {"part": ["id,snippet"], "id": ["a,b"]}, "ctype": "", "body": ""}],
            "stdout": "ok\n", "stderr": "", "exit": 0, "files": {}}
    base.update(over)
    return base


def req(**over):
    r = dict(result()["requests"][0])
    r.update(over)
    return r


def test_identical_results_have_no_difference():
    assert diff.compare(result(), result(), CASE) == []


def test_part_split_only_touches_part():
    joined = result(requests=[req(query={"part": ["id,snippet"], "id": ["a,b"]})])
    repeated = result(requests=[req(query={"part": ["id", "snippet"], "id": ["a,b"]})])
    assert diff.compare(joined, repeated, CASE) == []
    ids_split = result(requests=[req(query={"part": ["id,snippet"], "id": ["a", "b"]})])
    assert diff.compare(joined, ids_split, CASE) == ['request 0 query: {"id": ["a,b"], "part": ["id", "snippet"]} != {"id": ["a", "b"], "part": ["id", "snippet"]}']


def test_changed_query_value_is_reported():
    out = diff.compare(result(), result(requests=[req(query={"part": ["id,snippet"], "id": ["a,c"]})]), CASE)
    assert out == ['request 0 query: {"id": ["a,b"], "part": ["id", "snippet"]} != {"id": ["a,c"], "part": ["id", "snippet"]}']


def test_json_body_compares_by_value_and_reports_changed_field():
    a = result(requests=[req(method="POST", ctype="application/json", body='{"a":1,"b":{"c":"x"}}')])
    b = result(requests=[req(method="POST", ctype="application/json", body='{"b":{"c":"x"},"a":1}')])
    c = result(requests=[req(method="POST", ctype="application/json", body='{"b":{"c":"y"},"a":1}')])
    assert diff.compare(a, b, CASE) == []
    assert diff.compare(a, c, CASE) == ['request 0 body: {"a": 1, "b": {"c": "x"}} != {"b": {"c": "y"}, "a": 1}']


def test_multipart_boundary_is_masked_but_payload_is_not():
    def mp(boundary, media):
        body = (f"--{boundary}\r\nContent-Type: application/json\r\n\r\n{{\"x\":1}}\r\n"
                f"--{boundary}\r\nContent-Type: video/mp4\r\n\r\n{media}\r\n--{boundary}--\r\n")
        return result(requests=[req(method="POST", ctype=f"multipart/related; boundary={boundary}", body=body)])
    assert diff.compare(mp("aaa111", "DATA"), mp("bbb222", "DATA"), CASE) == []
    assert len(diff.compare(mp("aaa111", "DATA"), mp("bbb222", "DATB"), CASE)) == 1


def test_changed_stdout_byte_is_reported():
    out = diff.compare(result(), result(stdout="ok \n"), CASE)
    assert out == ["stdout: at byte 2: 'ok\\n' != 'ok \\n'"]


def test_slog_time_masked_only_in_stderr():
    a = result(stderr="time=2026-01-01T00:00:00Z level=INFO msg=x\n")
    b = result(stderr="time=2027-05-05T11:11:11Z level=INFO msg=x\n")
    assert diff.compare(a, b, CASE) == []
    assert len(diff.compare(result(stdout="time=1\n"), result(stdout="time=2\n"), CASE)) == 1


def test_port_masked():
    a = result(stderr='Get "http://127.0.0.1:5000/x"')
    b = result(stderr='Get "http://127.0.0.1:6000/x"')
    assert diff.compare(a, b, CASE) == []


def test_exit_code_and_request_count_reported():
    assert diff.compare(result(), result(exit=1), CASE) == ["exit 0 != 1"]
    assert diff.compare(result(), result(requests=[]), CASE) == ["request count 1 != 0"]


def test_mcp_stdout_compares_by_value():
    case = {"id": "m", "surface": "mcp"}
    a = result(stdout='{"id":1,"result":{"a":1,"b":2}}\n')
    b = result(stdout='{"result":{"b":2,"a":1},"id":1}\n')
    c = result(stdout='{"result":{"b":3,"a":1},"id":1}\n')
    assert diff.compare(a, b, case) == []
    assert len(diff.compare(a, c, case)) == 1


def test_unregistered_divergence_is_rejected():
    try:
        diff.compare(result(), result(), {"id": "x", "divergences": ["E999"]})
    except ValueError as e:
        assert "E999" in str(e)
    else:
        raise AssertionError("unregistered divergence accepted")
