"""add alarm toggle events

Revision ID: 2c76ad58d077
Revises: 44e33c5ee6a8
Create Date: 2026-09-14 15:05:38.355627

"""

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

# revision identifiers, used by Alembic.
revision: str = "2c76ad58d077"
down_revision: str | Sequence[str] | None = "44e33c5ee6a8"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.create_table(
        "alarm_toggle_events",
        sa.Column("id", sa.Integer(), autoincrement=True, nullable=False),
        sa.Column("enabled", sa.Boolean(), nullable=False),
        sa.Column("changed_at", sa.DateTime(timezone=True), nullable=False),
        sa.PrimaryKeyConstraint("id"),
    )
    op.create_index(
        op.f("ix_alarm_toggle_events_changed_at"),
        "alarm_toggle_events",
        ["changed_at"],
        unique=False,
    )


def downgrade() -> None:
    op.drop_index(
        op.f("ix_alarm_toggle_events_changed_at"), table_name="alarm_toggle_events"
    )
    op.drop_table("alarm_toggle_events")
