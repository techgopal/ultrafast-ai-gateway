from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from typing_extensions import Self

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.alert_channel_entry import AlertChannelEntry
    from ..models.alert_rule_entry import AlertRuleEntry
    from ..models.budget_entry import BudgetEntry
    from ..models.guardrail_entry import GuardrailEntry
    from ..models.limit_entry import LimitEntry
    from ..models.model_entry import ModelEntry
    from ..models.provider_entry import ProviderEntry
    from ..models.route_entry import RouteEntry
    from ..models.settings_entry import SettingsEntry
    from ..models.team_entry import TeamEntry


T = TypeVar("T", bound="ConfigFile")


@_attrs_define
class ConfigFile:
    """The configuration file. `format` and `version` come first.

    Attributes:
        format_ (str): Always `ultrafast-config`.
        version (int): Always 1.
        alert_channels (list[AlertChannelEntry] | Unset): Left out of the file when there are none.
        alert_rules (list[AlertRuleEntry] | Unset):
        budgets (list[BudgetEntry] | Unset):
        guardrails (list[GuardrailEntry] | Unset): Left out of the file when there are none.
        limits (list[LimitEntry] | Unset):
        models (list[ModelEntry] | Unset):
        providers (list[ProviderEntry] | Unset):
        routes (list[RouteEntry] | Unset):
        settings (SettingsEntry | Unset):
        teams (list[TeamEntry] | Unset):
    """

    format_: str
    version: int
    alert_channels: list[AlertChannelEntry] | Unset = UNSET
    alert_rules: list[AlertRuleEntry] | Unset = UNSET
    budgets: list[BudgetEntry] | Unset = UNSET
    guardrails: list[GuardrailEntry] | Unset = UNSET
    limits: list[LimitEntry] | Unset = UNSET
    models: list[ModelEntry] | Unset = UNSET
    providers: list[ProviderEntry] | Unset = UNSET
    routes: list[RouteEntry] | Unset = UNSET
    settings: SettingsEntry | Unset = UNSET
    teams: list[TeamEntry] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        format_ = self.format_

        version = self.version

        alert_channels: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.alert_channels, Unset):
            alert_channels = []
            for alert_channels_item_data in self.alert_channels:
                alert_channels_item = alert_channels_item_data.to_dict()
                alert_channels.append(alert_channels_item)

        alert_rules: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.alert_rules, Unset):
            alert_rules = []
            for alert_rules_item_data in self.alert_rules:
                alert_rules_item = alert_rules_item_data.to_dict()
                alert_rules.append(alert_rules_item)

        budgets: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.budgets, Unset):
            budgets = []
            for budgets_item_data in self.budgets:
                budgets_item = budgets_item_data.to_dict()
                budgets.append(budgets_item)

        guardrails: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.guardrails, Unset):
            guardrails = []
            for guardrails_item_data in self.guardrails:
                guardrails_item = guardrails_item_data.to_dict()
                guardrails.append(guardrails_item)

        limits: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.limits, Unset):
            limits = []
            for limits_item_data in self.limits:
                limits_item = limits_item_data.to_dict()
                limits.append(limits_item)

        models: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.models, Unset):
            models = []
            for models_item_data in self.models:
                models_item = models_item_data.to_dict()
                models.append(models_item)

        providers: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.providers, Unset):
            providers = []
            for providers_item_data in self.providers:
                providers_item = providers_item_data.to_dict()
                providers.append(providers_item)

        routes: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.routes, Unset):
            routes = []
            for routes_item_data in self.routes:
                routes_item = routes_item_data.to_dict()
                routes.append(routes_item)

        settings: dict[str, Any] | Unset = UNSET
        if not isinstance(self.settings, Unset):
            settings = self.settings.to_dict()

        teams: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.teams, Unset):
            teams = []
            for teams_item_data in self.teams:
                teams_item = teams_item_data.to_dict()
                teams.append(teams_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "format": format_,
                "version": version,
            }
        )
        if alert_channels is not UNSET:
            field_dict["alert_channels"] = alert_channels
        if alert_rules is not UNSET:
            field_dict["alert_rules"] = alert_rules
        if budgets is not UNSET:
            field_dict["budgets"] = budgets
        if guardrails is not UNSET:
            field_dict["guardrails"] = guardrails
        if limits is not UNSET:
            field_dict["limits"] = limits
        if models is not UNSET:
            field_dict["models"] = models
        if providers is not UNSET:
            field_dict["providers"] = providers
        if routes is not UNSET:
            field_dict["routes"] = routes
        if settings is not UNSET:
            field_dict["settings"] = settings
        if teams is not UNSET:
            field_dict["teams"] = teams

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.alert_channel_entry import AlertChannelEntry
        from ..models.alert_rule_entry import AlertRuleEntry
        from ..models.budget_entry import BudgetEntry
        from ..models.guardrail_entry import GuardrailEntry
        from ..models.limit_entry import LimitEntry
        from ..models.model_entry import ModelEntry
        from ..models.provider_entry import ProviderEntry
        from ..models.route_entry import RouteEntry
        from ..models.settings_entry import SettingsEntry
        from ..models.team_entry import TeamEntry

        d = dict(src_dict)
        format_ = d.pop("format")

        version = d.pop("version")

        _alert_channels = d.pop("alert_channels", UNSET)
        alert_channels: list[AlertChannelEntry] | Unset = UNSET
        if _alert_channels is not UNSET:
            alert_channels = []
            for alert_channels_item_data in _alert_channels:
                alert_channels_item = AlertChannelEntry.from_dict(
                    alert_channels_item_data
                )

                alert_channels.append(alert_channels_item)

        _alert_rules = d.pop("alert_rules", UNSET)
        alert_rules: list[AlertRuleEntry] | Unset = UNSET
        if _alert_rules is not UNSET:
            alert_rules = []
            for alert_rules_item_data in _alert_rules:
                alert_rules_item = AlertRuleEntry.from_dict(alert_rules_item_data)

                alert_rules.append(alert_rules_item)

        _budgets = d.pop("budgets", UNSET)
        budgets: list[BudgetEntry] | Unset = UNSET
        if _budgets is not UNSET:
            budgets = []
            for budgets_item_data in _budgets:
                budgets_item = BudgetEntry.from_dict(budgets_item_data)

                budgets.append(budgets_item)

        _guardrails = d.pop("guardrails", UNSET)
        guardrails: list[GuardrailEntry] | Unset = UNSET
        if _guardrails is not UNSET:
            guardrails = []
            for guardrails_item_data in _guardrails:
                guardrails_item = GuardrailEntry.from_dict(guardrails_item_data)

                guardrails.append(guardrails_item)

        _limits = d.pop("limits", UNSET)
        limits: list[LimitEntry] | Unset = UNSET
        if _limits is not UNSET:
            limits = []
            for limits_item_data in _limits:
                limits_item = LimitEntry.from_dict(limits_item_data)

                limits.append(limits_item)

        _models = d.pop("models", UNSET)
        models: list[ModelEntry] | Unset = UNSET
        if _models is not UNSET:
            models = []
            for models_item_data in _models:
                models_item = ModelEntry.from_dict(models_item_data)

                models.append(models_item)

        _providers = d.pop("providers", UNSET)
        providers: list[ProviderEntry] | Unset = UNSET
        if _providers is not UNSET:
            providers = []
            for providers_item_data in _providers:
                providers_item = ProviderEntry.from_dict(providers_item_data)

                providers.append(providers_item)

        _routes = d.pop("routes", UNSET)
        routes: list[RouteEntry] | Unset = UNSET
        if _routes is not UNSET:
            routes = []
            for routes_item_data in _routes:
                routes_item = RouteEntry.from_dict(routes_item_data)

                routes.append(routes_item)

        _settings = d.pop("settings", UNSET)
        settings: SettingsEntry | Unset
        if isinstance(_settings, Unset):
            settings = UNSET
        else:
            settings = SettingsEntry.from_dict(_settings)

        _teams = d.pop("teams", UNSET)
        teams: list[TeamEntry] | Unset = UNSET
        if _teams is not UNSET:
            teams = []
            for teams_item_data in _teams:
                teams_item = TeamEntry.from_dict(teams_item_data)

                teams.append(teams_item)

        config_file = cls(
            format_=format_,
            version=version,
            alert_channels=alert_channels,
            alert_rules=alert_rules,
            budgets=budgets,
            guardrails=guardrails,
            limits=limits,
            models=models,
            providers=providers,
            routes=routes,
            settings=settings,
            teams=teams,
        )

        return config_file
