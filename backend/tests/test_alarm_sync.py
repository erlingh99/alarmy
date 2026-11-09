from .test_alarm_api import make_alarm_payload


async def test_sync_returns_alarms_and_flag_together(client):
    response = await client.get("/api/alarms/sync")
    assert response.status_code == 200

    body = response.json()
    assert body == {"alarms": [], "alarms_enabled": True, "has_more": False}


async def test_sync_reflects_disabled_state(client):
    await client.put("/api/mode/alarms", json={"enabled": False})

    assert (await client.get("/api/alarms/sync")).json()["alarms_enabled"] is False


async def test_sync_route_is_not_shadowed_by_the_id_route(client):
    # /api/alarms/{alarm_id} takes a UUID, so if /sync were declared after it
    # this would be a 422 rather than a 200.
    assert (await client.get("/api/alarms/sync")).status_code == 200


async def test_sync_includes_inactive_alarms_by_default(client):
    await client.put("/api/alarms", json=make_alarm_payload(is_active=False))

    assert len((await client.get("/api/alarms/sync")).json()["alarms"]) == 1


async def test_sync_only_active_leaves_out_inactive_alarms(client):
    await client.put("/api/alarms", json=make_alarm_payload(is_active=False))

    response = await client.get("/api/alarms/sync", params={"only_active": True})
    assert response.json()["alarms"] == []


async def test_sync_pages_through_all_alarms_by_id(client):
    ids = []
    for _ in range(5):
        payload = make_alarm_payload()
        ids.append(payload["id"])
        await client.put("/api/alarms", json=payload)

    seen = []
    params = {"limit": 2}
    while True:
        body = (await client.get("/api/alarms/sync", params=params)).json()
        seen += [alarm["id"] for alarm in body["alarms"]]
        if not body["has_more"]:
            break
        params["after"] = seen[-1]

    assert seen == sorted(ids)


async def test_sync_last_full_page_has_no_more(client):
    for _ in range(2):
        await client.put("/api/alarms", json=make_alarm_payload())

    body = (await client.get("/api/alarms/sync", params={"limit": 2})).json()
    assert len(body["alarms"]) == 2
    assert body["has_more"] is False
