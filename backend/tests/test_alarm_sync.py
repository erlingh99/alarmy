async def test_sync_returns_alarms_and_flag_together(client):
    response = await client.get("/api/alarms/sync")
    assert response.status_code == 200

    body = response.json()
    assert body == {"alarms": [], "alarms_enabled": True}


async def test_sync_reflects_disabled_state(client):
    await client.put("/api/mode/alarms", json={"enabled": False})

    assert (await client.get("/api/alarms/sync")).json()["alarms_enabled"] is False


async def test_sync_route_is_not_shadowed_by_the_id_route(client):
    # /api/alarms/{alarm_id} takes a UUID, so if /sync were declared after it
    # this would be a 422 rather than a 200.
    assert (await client.get("/api/alarms/sync")).status_code == 200
