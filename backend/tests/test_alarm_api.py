from uuid import uuid4


def make_alarm_payload(**overrides) -> dict:
    base = {
        "id": str(uuid4()),
        "name": "Test Alarm",
        "time": "07:00:00",
        "color": "#ff5500",
        "length": 30,
        "intensity_curve": {
            "start_intensity": 0,
            "end_intensity": 100,
            "curve": "linear",
        },
        "is_active": True,
        "recurrence": {"recurrence_type": "daily"},
    }
    base.update(overrides)
    return base


class TestGetAlarms:
    async def test_get_alarms_empty(self, client):
        response = await client.get("/api/alarms")
        assert response.status_code == 200
        assert response.json() == []

    async def test_get_alarms_after_create(self, client):
        payload = make_alarm_payload()
        await client.put("/api/alarms", json=payload)
        response = await client.get("/api/alarms")
        assert response.status_code == 200
        assert len(response.json()) == 1


class TestGetAlarmById:
    async def test_get_existing_alarm(self, client):
        payload = make_alarm_payload()
        await client.put("/api/alarms", json=payload)
        response = await client.get(f"/api/alarms/{payload['id']}")
        assert response.status_code == 200
        assert response.json()["name"] == "Test Alarm"

    async def test_get_nonexistent_alarm(self, client):
        response = await client.get(f"/api/alarms/{uuid4()}")
        assert response.status_code == 404
        assert "not found" in response.json()["detail"].lower()


class TestDeleteAlarm:
    async def test_delete_existing_alarm(self, client):
        payload = make_alarm_payload()
        await client.put("/api/alarms", json=payload)
        response = await client.delete(f"/api/alarms/{payload['id']}")
        assert response.status_code == 204
        # Confirm deletion
        get_response = await client.get(f"/api/alarms/{payload['id']}")
        assert get_response.status_code == 404

    async def test_delete_nonexistent_alarm(self, client):
        # Deleting a non-existent alarm should not error (idempotent)
        response = await client.delete(f"/api/alarms/{uuid4()}")
        assert response.status_code == 204
