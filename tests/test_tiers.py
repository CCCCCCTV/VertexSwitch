import pytest

from vertexswitch.core.tiers import (
    HEADER_REQUEST_TYPE,
    HEADER_SERVER_TIMEOUT,
    HEADER_SHARED_TYPE,
    Tier,
    TrafficTypeWatcher,
    check_traffic_type,
    parse_tier,
    plan_tier,
)

ARGON = frozenset({"gemini-4-argon"})


def _plan(tier, model="gemini-3.5-flash", region="global", **kw):
    kw.setdefault("flex_unsupported_models", ARGON)
    return plan_tier(tier, model=model, region=region, **kw)


def test_standard_sends_nothing():
    p = _plan(Tier.STANDARD)
    assert p.headers == {}
    assert p.effective is Tier.STANDARD
    assert not p.sent_tier_header


def test_standard_shared_only():
    assert _plan(Tier.STANDARD, shared_only=True).headers == {HEADER_REQUEST_TYPE: "shared"}


def test_flex_headers_and_timeout():
    p = _plan(Tier.FLEX, flex_timeout=1800)
    assert p.headers == {HEADER_SHARED_TYPE: "flex", HEADER_SERVER_TIMEOUT: "1800"}
    assert p.server_timeout == 1800
    assert p.warnings == ()


def test_flex_timeout_capped_at_30_minutes():
    p = _plan(Tier.FLEX, flex_timeout=99999)
    assert p.headers[HEADER_SERVER_TIMEOUT] == "1800"


def test_flex_shared_only():
    p = _plan(Tier.FLEX, shared_only=True, flex_timeout=600)
    assert p.headers == {
        HEADER_REQUEST_TYPE: "shared",
        HEADER_SHARED_TYPE: "flex",
        HEADER_SERVER_TIMEOUT: "600",
    }


@pytest.mark.parametrize("region", ["us", "eu", "us-central1"])
def test_flex_non_global_downgrades_without_any_header(region):
    p = _plan(Tier.FLEX, region=region, shared_only=True)
    assert p.effective is Tier.STANDARD
    assert p.headers == {}
    assert p.server_timeout is None
    assert p.warnings and "global" in p.warnings[0]


def test_flex_unsupported_model_downgrades_without_any_header():
    p = _plan(Tier.FLEX, model="gemini-4-argon", shared_only=True)
    assert p.effective is Tier.STANDARD
    assert p.headers == {}
    assert "gemini-4-argon" in p.warnings[0]


@pytest.mark.parametrize("region", ["global", "us", "eu"])
def test_priority_supported_regions(region):
    p = _plan(Tier.PRIORITY, region=region)
    assert p.headers == {HEADER_SHARED_TYPE: "priority"}
    assert p.warnings == ()


def test_priority_on_flex_unsupported_model_is_fine():
    assert _plan(Tier.PRIORITY, model="gemini-4-argon").effective is Tier.PRIORITY


def test_priority_regional_downgrades():
    p = _plan(Tier.PRIORITY, region="us-east5")
    assert p.effective is Tier.STANDARD
    assert p.headers == {}
    assert p.warnings


def test_parse_tier():
    assert parse_tier("FLEX") is Tier.FLEX
    assert parse_tier(" priority ") is Tier.PRIORITY
    assert parse_tier("bogus") is None
    assert parse_tier(None) is None


@pytest.mark.parametrize(
    ("tier", "shared_only", "traffic", "warns"),
    [
        (Tier.FLEX, False, "ON_DEMAND_FLEX", False),
        (Tier.FLEX, False, "ON_DEMAND", True),
        (Tier.FLEX, False, "PROVISIONED_THROUGHPUT", False),
        (Tier.FLEX, True, "PROVISIONED_THROUGHPUT", True),
        (Tier.PRIORITY, False, "ON_DEMAND_PRIORITY", False),
        (Tier.PRIORITY, False, "ON_DEMAND", True),
        (Tier.STANDARD, False, "ON_DEMAND", False),
        (Tier.STANDARD, True, "PROVISIONED_THROUGHPUT", False),
    ],
)
def test_check_traffic_type(tier, shared_only, traffic, warns):
    p = _plan(tier, shared_only=shared_only)
    assert (check_traffic_type(p, traffic) is not None) is warns


def test_no_check_when_downgraded():
    p = _plan(Tier.FLEX, region="us")
    assert check_traffic_type(p, "ON_DEMAND") is None


def test_traffic_watcher_split_chunks_reports_once():
    w = TrafficTypeWatcher()
    assert w.feed(b'data: {"usageMetadata": {"traffic') is None
    assert w.feed(b'Type": "ON_DEMAND_FLEX"}}\r\n\r\n') == "ON_DEMAND_FLEX"
    assert w.feed(b'data: {"usageMetadata": {"trafficType": "ON_DEMAND"}}') is None
    assert w.traffic_type == "ON_DEMAND_FLEX"
