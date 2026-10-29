"""Tests for Pydantic model validation."""

from datetime import time

import pytest
from pydantic import ValidationError

from app.models.alarm import (
    Alarm,
    AlarmUpdate,
    ControlPoint,
    IntensityCurve,
    RecurrencePattern,
)


def make_alarm(**overrides) -> dict:
    base = {
        "name": "Morning",
        "time": time(7, 0),
        "color": "#ff5500",
        "length": 30,
        "intensity_curve": IntensityCurve(
            start_intensity=0,
            end_intensity=100,
            curve="linear",
        ),
        "is_active": True,
        "recurrence": RecurrencePattern(recurrence_type="daily"),
    }
    base.update(overrides)
    return base


class TestAlarm:
    def test_valid_alarm(self):
        alarm = Alarm(**make_alarm())
        assert alarm.name == "Morning"
        assert alarm.color == "#ff5500"

    def test_name_too_long(self):
        with pytest.raises(ValidationError):
            Alarm(**make_alarm(name="a" * 33))

    def test_name_empty(self):
        with pytest.raises(ValidationError):
            Alarm(**make_alarm(name=""))

    def test_invalid_color_format(self):
        with pytest.raises(ValidationError):
            Alarm(**make_alarm(color="red"))

    def test_invalid_color_short(self):
        with pytest.raises(ValidationError):
            Alarm(**make_alarm(color="#fff"))

    def test_length_bounds(self):
        with pytest.raises(ValidationError):
            Alarm(**make_alarm(length=0))
        with pytest.raises(ValidationError):
            Alarm(**make_alarm(length=256))

    def test_length_valid_bounds(self):
        alarm = Alarm(**make_alarm(length=1))
        assert alarm.length == 1
        alarm = Alarm(**make_alarm(length=255))
        assert alarm.length == 255


class TestIntensityCurve:
    def test_valid_linear(self):
        curve = IntensityCurve(start_intensity=0, end_intensity=100, curve="linear")
        assert curve.curve == "linear"

    def test_intensity_out_of_range(self):
        with pytest.raises(ValidationError):
            IntensityCurve(start_intensity=-1, end_intensity=100, curve="linear")
        with pytest.raises(ValidationError):
            IntensityCurve(start_intensity=0, end_intensity=101, curve="linear")

    def test_too_many_control_points(self):
        points = [ControlPoint(x=float(i * 10), y=float(i * 10)) for i in range(6)]
        with pytest.raises(ValidationError):
            IntensityCurve(
                start_intensity=0,
                end_intensity=100,
                curve="custom",
                control_points=points,
            )

    def test_five_control_points_valid(self):
        points = [ControlPoint(x=float(i * 10), y=float(i * 10)) for i in range(5)]
        curve = IntensityCurve(
            start_intensity=0,
            end_intensity=100,
            curve="custom",
            control_points=points,
        )
        assert curve.control_points is not None
        assert len(curve.control_points) == 5


class TestControlPoint:
    def test_valid(self):
        cp = ControlPoint(x=50.0, y=75.0)
        assert cp.x == 50.0

    def test_x_out_of_range(self):
        with pytest.raises(ValidationError):
            ControlPoint(x=-1.0, y=50.0)
        with pytest.raises(ValidationError):
            ControlPoint(x=101.0, y=50.0)

    def test_y_out_of_range(self):
        with pytest.raises(ValidationError):
            ControlPoint(x=50.0, y=-1.0)
        with pytest.raises(ValidationError):
            ControlPoint(x=50.0, y=101.0)


class TestRecurrencePattern:
    def test_daily(self):
        r = RecurrencePattern(recurrence_type="daily")
        assert r.recurrence_type == "daily"

    def test_weekly_with_days(self):
        r = RecurrencePattern(
            recurrence_type="weekly",
            days=(True, False, True, False, True, False, False),
        )
        assert r.days is not None
        assert r.days[0] is True

    def test_too_many_custom_dates(self):
        from datetime import date

        dates = [date(2026, 1, i) for i in range(1, 7)]
        with pytest.raises(ValidationError):
            RecurrencePattern(recurrence_type="custom", custom_dates=dates)


class TestAlarmUpdate:
    def test_partial_update_name_only(self):
        update = AlarmUpdate(name="Evening")
        assert update.name == "Evening"
        assert update.color is None

    def test_invalid_color_in_update(self):
        with pytest.raises(ValidationError):
            AlarmUpdate(color="notacolor")

    def test_empty_update(self):
        update = AlarmUpdate()
        assert update.name is None
